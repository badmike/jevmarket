<script setup lang="ts">
import { useNow } from '@vueuse/core'

import { useLive } from '~/api/live'
import { PulseDot } from '~/components/ui/pulse-dot'
import { SimpleTooltip } from '~/components/ui/tooltip'

const { connection, retryAt, reconnect } = useLive()
const now = useNow({ interval: 1000 })

const label = computed(() => {
  if (connection.value === 'open') return 'Live'
  if (connection.value === 'connecting') return 'Connecting'
  const secs = retryAt.value ? Math.max(0, Math.ceil((retryAt.value - now.value.getTime()) / 1000)) : 0
  return secs > 0 ? `Offline, retry in ${secs}s` : 'Reconnecting'
})

const tooltip = computed(() =>
  connection.value === 'open'
    ? 'Live: receiving updates from the daemon'
    : `${label.value}: not receiving updates. Click to reconnect now.`
)
</script>

<template>
  <SimpleTooltip
    :tooltip="tooltip"
    class="flex size-8 cursor-pointer items-center justify-center rounded-md transition-colors hover:bg-secondary/80"
    :aria-label="`Live updates: ${label}`"
    aria-live="polite"
    @click="connection !== 'open' && reconnect()"
  >
    <PulseDot
      size="sm"
      :variant="connection === 'open' ? 'success' : connection === 'connecting' ? 'default' : 'destructive'"
      :live="connection === 'open'"
    />
  </SimpleTooltip>
</template>
