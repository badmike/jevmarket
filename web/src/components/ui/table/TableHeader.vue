<script setup lang="ts">
import type { HTMLAttributes } from 'vue'

import { cn } from '~/lib/utils'

/**
 * The column row. A fill one step off the table's own surface, closed by a
 * stronger rule: enough to read as a header against the table's border
 * without the shouting of a dark bar. The row takes no hover, because it is
 * not a record.
 */
const props = defineProps<{
  class?: HTMLAttributes['class']
}>()
</script>

<template>
  <thead
    role="rowgroup"
    :class="
      cn(
        'bg-muted-background/50 [&_tr]:border-b [&_tr]:border-b-border-strong [&_tr:hover]:bg-transparent',
        props.class
      )
    "
  >
    <slot />
  </thead>
</template>
