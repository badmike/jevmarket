<script setup lang="ts">
import type { HTMLAttributes } from 'vue'

import { cn } from '~/lib/utils'

const props = defineProps<{
  class?: HTMLAttributes['class']
}>()
</script>

<template>
  <tr
    role="row"
    :class="
      cn(
        'border-b border-b-border transition-colors duration-150 ease-butter last:border-b-0 hover:bg-secondary/50 data-[state=selected]:bg-secondary/70',
        props.class
      )
    "
  >
    <slot />
  </tr>
</template>
