<script setup lang="ts">
import { Badge, type BadgeVariants } from '~/components/ui/badge'

const props = defineProps<{ status: string; dryRun: boolean }>()

const variant = computed<BadgeVariants['variant']>(() => {
  if (props.dryRun) return 'secondary'
  if (['rejected', 'failed', 'refused'].includes(props.status)) return 'destructive'
  return props.status === 'matched' ? 'success' : 'info'
})
</script>

<template>
  <Badge
    :variant="variant"
    size="sm"
    class="whitespace-nowrap"
  >
    {{ dryRun ? 'Dry run' : status }}
  </Badge>
</template>
