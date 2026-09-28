<script setup lang="ts">
import type { HTMLAttributes } from 'vue'

import { cn } from '~/lib/utils'

/**
 * A column label. Quieter than the data under it, but not faint: the weight,
 * the header's fill and the stronger rule under the row are what separate the
 * header from the first record without spending vertical space on it.
 */
const props = defineProps<{
  class?: HTMLAttributes['class']
}>()
</script>

<template>
  <th
    scope="col"
    role="columnheader"
    :class="
      cn(
        'h-9 px-3 text-left align-middle text-xs font-semibold whitespace-nowrap text-muted [&:has([role=checkbox])]:pr-0 [&>[role=checkbox]]:translate-y-0.5',
        props.class
      )
    "
  >
    <slot />
  </th>
</template>
