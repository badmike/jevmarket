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
    ? 'Receiving live updates from the daemon'
    : 'Not receiving updates. Click to reconnect now.'
)
</script>

<template>
  <SimpleTooltip
    :tooltip="tooltip"
    class="flex h-8 cursor-pointer items-center gap-2 rounded-md px-2 text-xs font-medium text-muted transition-colors hover:bg-secondary/80"
    :aria-label="`Live updates: ${label}`"
    @click="connection !== 'open' && reconnect()"
  >
    <PulseDot
      size="sm"
      :variant="connection === 'open' ? 'success' : connection === 'connecting' ? 'default' : 'destructive'"
      :live="connection === 'open'"
    />
    <span
      class="hidden @lg:inline"
      aria-live="polite"
    >
      {{ label }}
    </span>
  </SimpleTooltip>
</template>
