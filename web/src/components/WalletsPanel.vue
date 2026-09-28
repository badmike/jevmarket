<script setup lang="ts">
import { useWallets } from '~/api/queries'
import type { Positions, WalletKind } from '~/api/types'
import ExposureGauge from '~/components/ExposureGauge.vue'
import Icon from '~/components/Icon.vue'
import StatTile from '~/components/StatTile.vue'
import TransferDialog from '~/components/TransferDialog.vue'
import { Alert, AlertDescription } from '~/components/ui/alert'
import { Skeleton } from '~/components/ui/skeleton'
import { SimpleTooltip } from '~/components/ui/tooltip'
import { usd } from '~/lib/format'

/**
 * The money at a glance: everything held, what is at risk against the cap,
 * and both wallets with the transfers between them.
 */
const props = defineProps<{
  positions: Positions | undefined
  /** The positions are still loading; without it, missing positions mean unreadable. */
  loading: boolean
}>()

const { data, error, refetch } = useWallets()

const rows: { kind: WalletKind; title: string; help: string }[] = [
  { kind: 'deposit', title: 'Deposit wallet', help: "Places the bot's orders" },
  { kind: 'proxy', title: 'Polymarket.com wallet', help: 'Your account on the site' },
]

/** Cash in both wallets, or the trading wallet alone when the wallets cannot be read. */
const cash = computed(() =>
  data.value ? data.value.deposit.balance_usd + data.value.proxy.balance_usd : (props.positions?.balance_usd ?? null)
)
const total = computed(() =>
  props.positions && cash.value != null ? cash.value + props.positions.exposure.positions_usd : null
)
const headroom = computed(() => {
  const e = props.positions?.exposure
  return e ? e.cap_usd - e.total_usd : null
})
const walletsPending = computed(() => !data.value && !error.value)
</script>

<template>
  <section
    class="grid gap-3"
    aria-label="Money"
  >
    <Alert
      v-for="w in data?.warnings ?? []"
      :key="w"
      color="warning"
      icon="lucide:triangle-alert"
    >
      <AlertDescription>{{ w }}</AlertDescription>
    </Alert>

    <div class="grid gap-3 sm:grid-cols-2">
      <Skeleton
        v-if="loading || walletsPending"
        class="h-32 rounded-xl"
      />
      <StatTile
        v-else
        title="Total"
        icon="lucide:piggy-bank"
      >
        {{ usd(total) }}
        <template #detail>
          <template v-if="total == null">Wallet not readable</template>
          <template v-else>
            {{ usd(cash) }} {{ data ? 'cash' : 'in the deposit wallet' }},
            {{ usd(positions?.exposure.positions_usd) }} in positions
          </template>
        </template>
      </StatTile>

      <Skeleton
        v-if="loading"
        class="h-32 rounded-xl"
      />
      <StatTile
        v-else
        title="At risk"
        hint="Money in positions plus buy orders still waiting to fill. At the cap set in Settings, the bot stops buying."
        icon="lucide:gauge"
      >
        <ExposureGauge
          v-if="positions"
          :exposure="positions.exposure"
        />
        <template v-else>{{ usd(null) }}</template>
        <template
          v-if="headroom != null"
          #detail
        >
          <template v-if="headroom > 0">The bot can commit {{ usd(headroom) }} more</template>
          <span
            v-else
            class="text-warning-foreground"
          >
            At the cap, the bot places no new orders
          </span>
        </template>
      </StatTile>

      <template v-if="data">
        <div
          v-for="row in rows"
          :key="row.kind"
          class="grid content-start gap-1 rounded-xl bg-card p-4 shadow-soft"
        >
          <div class="flex items-center justify-between gap-2">
            <h2 class="text-sm font-medium text-muted">{{ row.title }}</h2>
            <a
              :href="`https://polymarket.com/profile/${data[row.kind].address}`"
              target="_blank"
              rel="noopener noreferrer"
              class="inline-flex items-center gap-1 font-mono text-xs whitespace-nowrap text-accent hover:underline"
              :title="data[row.kind].address"
            >
              {{ data[row.kind].address.slice(0, 6) }}…{{ data[row.kind].address.slice(-4) }}
              <Icon
                name="lucide:external-link"
                size="12"
                aria-hidden="true"
              />
            </a>
          </div>
          <div class="flex flex-wrap items-center justify-between gap-2">
            <p class="text-xl font-semibold text-primary tabular-nums">
              {{ usd(data[row.kind].balance_usd) }}
              <SimpleTooltip
                tooltip="pUSD is the dollar token Polymarket trades in. One pUSD is worth one US dollar."
                as-child
              >
                <span class="text-sm font-normal text-muted underline decoration-muted/50 decoration-dotted underline-offset-4">pUSD</span>
              </SimpleTooltip>
            </p>
            <TransferDialog
              :from="row.kind"
              :source="data[row.kind]"
              :target="data[row.kind === 'deposit' ? 'proxy' : 'deposit']"
              @moved="refetch()"
            />
          </div>
          <p class="text-xs text-muted">{{ row.help }}</p>
        </div>
      </template>
      <div
        v-else-if="error"
        class="grid content-center gap-2 rounded-xl bg-card p-4 text-sm text-muted shadow-soft sm:col-span-2"
      >
        <p>Could not read the wallets: {{ error.message }}</p>
        <p>
          <button
            type="button"
            class="text-accent hover:underline"
            @click="refetch()"
          >
            Retry
          </button>
        </p>
      </div>
      <template v-else>
        <Skeleton
          v-for="i in 2"
          :key="i"
          class="h-32 rounded-xl"
        />
      </template>
    </div>
  </section>
</template>
