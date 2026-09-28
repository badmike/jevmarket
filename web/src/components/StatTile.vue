<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import { SimpleTooltip } from '~/components/ui/tooltip'

defineProps<{
  title: string
  icon: string
  /** One line under the value. */
  detail?: string
  /** Explains the title in plain words, on hover. */
  hint?: string
}>()
</script>

<template>
  <div class="grid content-start gap-1 rounded-xl bg-card p-4 shadow-soft">
    <div class="flex items-center justify-between gap-2">
      <h2 class="text-sm font-medium text-muted">
        <SimpleTooltip
          v-if="hint"
          :tooltip="hint"
          as-child
        >
          <span class="underline decoration-muted/50 decoration-dotted underline-offset-4">{{ title }}</span>
        </SimpleTooltip>
        <template v-else>{{ title }}</template>
      </h2>
      <Icon
        :name="icon"
        size="16"
        class="text-muted"
        aria-hidden="true"
      />
    </div>
    <div class="text-xl font-semibold text-primary tabular-nums">
      <slot />
    </div>
    <p
      v-if="detail || $slots.detail"
      class="text-xs text-muted"
    >
      <slot name="detail">{{ detail }}</slot>
    </p>
  </div>
</template>
