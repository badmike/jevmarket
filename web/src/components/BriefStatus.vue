<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import { SimpleTooltip } from '~/components/ui/tooltip'

/** A brief's lifecycle as one small icon; the tooltip says what it means for the next pass. */
const props = defineProps<{ fresh: boolean; settled: boolean }>()

const status = computed(() => {
  if (props.settled)
    return {
      icon: 'lucide:archive',
      tone: 'text-muted',
      label: 'Settled: the market resolved or its end date passed',
    }
  if (props.fresh)
    return {
      icon: 'lucide:circle-check',
      tone: 'text-success',
      label: 'Fresh: passes reuse this brief until research_ttl_hours runs out',
    }
  return {
    icon: 'lucide:clock-alert',
    tone: 'text-warning',
    label: 'Expired: the next decision on this market researches again',
  }
})
</script>

<template>
  <SimpleTooltip
    :tooltip="status.label"
    as-child
  >
    <span
      role="img"
      :aria-label="status.label"
      class="inline-flex shrink-0 rounded-sm"
      :class="status.tone"
    >
      <Icon
        :name="status.icon"
        size="14"
        aria-hidden="true"
      />
    </span>
  </SimpleTooltip>
</template>
