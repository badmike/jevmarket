<script setup lang="ts">
import { useNow } from '@vueuse/core'

import { usePositions, useStatus } from '~/api/queries'
import ActivityFeed from '~/components/ActivityFeed.vue'
import ExposureGauge from '~/components/ExposureGauge.vue'
import StatTile from '~/components/StatTile.vue'
import { Alert, AlertDescription } from '~/components/ui/alert'
import { Card } from '~/components/ui/card'
import { Progress } from '~/components/ui/progress'
import { Skeleton } from '~/components/ui/skeleton'
import { errorMessage } from '~/api/client'
import { ago, countdown, usd } from '~/lib/format'
import type { Spend } from '~/api/types'

const { data: status, error } = useStatus()
const { data: positions, error: positionsError, isPending: positionsPending } = usePositions()
const now = useNow({ interval: 1000 })
const seconds = computed(() => now.value.getTime() / 1000)

const total = (s: Spend) => s.jev_usd + s.research_usd

const loop = computed(() => {
  const s = status.value
  if (!s) return null
  switch (s.state) {
    case 'running':
      return { label: 'Running a pass', detail: `Pass ${s.pass?.number ?? ''} started ${ago(s.pass?.started_at, seconds.value)}` }
    case 'stopping':
      return { label: 'Stopping', detail: 'Finishing the current market' }
    case 'paused':
      return { label: 'Paused', detail: 'Resume to schedule passes again' }
    case 'waiting':
      return {
        label: s.next_pass_at ? `Next pass in ${countdown(s.next_pass_at, seconds.value)}` : 'Waiting',
        detail: `Every ${Math.round(s.loop_secs / 60)} minutes`,
      }
  }
})

const pass = computed(() => status.value?.pass ?? status.value?.last_pass ?? null)
const progress = computed(() => {
  const p = pass.value
  if (!p || !p.candidates) return 0
  return (p.assessed / p.candidates) * 100
})
</script>

<template>
  <div class="mx-auto grid max-w-6xl gap-6">
    <header class="grid gap-1">
      <h1 class="text-2xl font-semibold text-primary">Overview</h1>
      <p class="text-sm text-muted">The trading loop, what it spends and what it holds, as it happens.</p>
    </header>

    <Alert
      v-if="error"
      color="destructive"
      icon="lucide:unplug"
    >
      <AlertDescription>Cannot reach the daemon: {{ errorMessage(error) }}</AlertDescription>
    </Alert>

    <Alert
      v-if="status?.last_error"
      color="warning"
      icon="lucide:triangle-alert"
    >
      <AlertDescription>
        <p class="font-semibold">The last pass failed</p>
        <p class="break-words">{{ status.last_error }}</p>
        <p
          v-if="status.state === 'paused'"
          class="mt-1"
        >
          The loop paused itself because every further pass would fail the same way. Fix the cause, then resume.
        </p>
      </AlertDescription>
    </Alert>

    <div class="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
      <template v-if="status && loop">
        <StatTile
          title="Loop"
          :icon="status.state === 'paused' ? 'lucide:pause' : 'lucide:repeat'"
          :detail="loop.detail"
        >
          {{ loop.label }}
        </StatTile>
        <StatTile
          title="Mode"
          :icon="status.dry_run ? 'lucide:flask-conical' : 'lucide:circle-dollar-sign'"
          :detail="
            status.dry_run_forced
              ? 'Forced by --dry-run'
              : status.dry_run
                ? 'Orders are logged, never placed'
                : 'Passes place real orders'
          "
        >
          <span :class="!status.dry_run && 'text-success'">{{ status.dry_run ? 'Dry run' : 'Live' }}</span>
        </StatTile>
        <StatTile
          title="OpenRouter spend"
          icon="lucide:coins"
          :detail="`${status.spend_total.jev_calls} Jev calls, ${status.spend_total.briefs} briefs, all time`"
        >
          {{ usd(total(status.spend_total), true) }}
        </StatTile>
      </template>
      <template v-else-if="!error">
        <Skeleton
          v-for="i in 3"
          :key="i"
          class="h-26 rounded-xl"
        />
      </template>
      <StatTile
        title="Exposure"
        icon="lucide:gauge"
      >
        <Skeleton
          v-if="positionsPending"
          class="h-14"
        />
        <p
          v-else-if="positionsError"
          class="text-sm font-normal text-muted"
        >
          Wallet not readable: {{ errorMessage(positionsError) }}
        </p>
        <ExposureGauge
          v-else-if="positions"
          :exposure="positions.exposure"
        />
      </StatTile>
    </div>

    <Card
      v-if="status"
      class="grid gap-3 p-4"
    >
      <div class="flex flex-wrap items-baseline justify-between gap-2">
        <h2 class="text-base font-semibold text-primary">
          <template v-if="status.pass">Pass {{ status.pass.number }} in progress</template>
          <template v-else-if="status.last_pass">Last pass, number {{ status.last_pass.number }}</template>
          <template v-else>No pass yet</template>
        </h2>
        <p
          v-if="pass"
          class="text-xs text-muted"
        >
          {{ pass.dry_run ? 'Dry run' : 'Live' }} · started {{ ago(pass.started_at, seconds) }}
          <template v-if="pass.finished_at">
            · took {{ Math.round(pass.finished_at - pass.started_at) }}s
          </template>
        </p>
      </div>
      <template v-if="pass">
        <Progress
          :model-value="progress"
          :aria-label="`${pass.assessed} of ${pass.candidates} markets assessed`"
        />
        <dl class="grid grid-cols-2 gap-3 text-sm sm:grid-cols-4">
          <div>
            <dt class="text-xs text-muted">Assessed</dt>
            <dd class="font-semibold text-primary tabular-nums">{{ pass.assessed }} of {{ pass.candidates }}</dd>
          </div>
          <div>
            <dt class="text-xs text-muted">Trades</dt>
            <dd class="font-semibold text-primary tabular-nums">{{ pass.trades }}</dd>
          </div>
          <div>
            <dt class="text-xs text-muted">Jev</dt>
            <dd class="font-semibold text-primary tabular-nums">
              {{ usd(pass.spend.jev_usd, true) }}
              <span class="font-normal text-muted">({{ pass.spend.jev_calls }} calls)</span>
            </dd>
          </div>
          <div>
            <dt class="text-xs text-muted">Research</dt>
            <dd class="font-semibold text-primary tabular-nums">
              {{ usd(pass.spend.research_usd, true) }}
              <span class="font-normal text-muted">({{ pass.spend.briefs }} briefs)</span>
            </dd>
          </div>
        </dl>
        <p
          v-if="pass.error && !status.pass"
          class="text-sm break-words text-destructive"
        >
          {{ pass.error }}
        </p>
      </template>
      <p
        v-else
        class="text-sm text-muted"
      >
        The first pass starts right after launch unless the loop is paused.
      </p>
    </Card>

    <ActivityFeed />
  </div>
</template>
