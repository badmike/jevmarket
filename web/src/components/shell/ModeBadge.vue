<script setup lang="ts">
import { useStatus } from '~/api/queries'
import Icon from '~/components/Icon.vue'
import { Badge } from '~/components/ui/badge'
import { SimpleTooltip } from '~/components/ui/tooltip'

const { data: status } = useStatus()

const tooltip = computed(() => {
  const s = status.value
  if (!s) return ''
  if (s.dry_run_forced) return 'Started with --dry-run: orders are only logged, whatever the config says'
  return s.dry_run ? 'Orders are only logged' : 'Passes place real orders on Polymarket'
})
</script>

<template>
  <SimpleTooltip
    v-if="status"
    :tooltip="tooltip"
    as-child
  >
    <Badge
      :variant="status.dry_run ? 'secondary' : 'success'"
      size="sm"
      class="gap-1"
      tabindex="0"
    >
      <Icon
        :name="status.dry_run ? 'lucide:flask-conical' : 'lucide:circle-dollar-sign'"
        size="12"
        aria-hidden="true"
      />
      {{ status.dry_run ? 'Dry run' : 'Live' }}
    </Badge>
  </SimpleTooltip>
</template>
