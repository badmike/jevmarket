<script setup lang="ts">
import type { Action } from '~/api/types'
import { Badge, type BadgeVariants } from '~/components/ui/badge'

const props = defineProps<{ action: Action }>()

interface Look {
  label: string
  variant: BadgeVariants['variant']
}

const looks: Record<string, Look> = {
  trade: { label: 'Trade', variant: 'success' },
  trade_unexecuted: { label: 'Trade, not placed', variant: 'warning' },
  skip: { label: 'Skip', variant: 'secondary' },
}

const look = computed<Look>(() => looks[props.action] ?? { label: props.action, variant: 'secondary' })
</script>

<template>
  <Badge
    :variant="look.variant"
    size="sm"
    class="whitespace-nowrap"
  >
    {{ look.label }}
  </Badge>
</template>
