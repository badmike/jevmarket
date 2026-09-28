<script setup lang="ts">
import type { HTMLAttributes } from 'vue'

import { cn } from '~/lib/utils'

/**
 * One cell. `label` is the column's name, repeated for the stacked layout a
 * phone gets: there the header row is off screen, so the cell has to say what
 * it is. It is real text rather than generated content, and it is hidden the
 * screen-reader way rather than removed, so a phone still announces it while
 * the record stays one line of values wide.
 *
 * The name is also written to `data-label`, which is what the stacked layout
 * tells a record's secondary fields from its subject and its actions by.
 *
 * `labelled` shows the name in the stacked layout, for cells whose value is a
 * bare number that means nothing without it.
 */
const props = defineProps<{
  class?: HTMLAttributes['class']
  label?: string
  labelled?: boolean
}>()
</script>

<template>
  <td
    role="cell"
    :data-label="label"
    :class="
      cn(
        'px-3 py-2 align-middle [&:has([role=checkbox])]:pr-0 [&>[role=checkbox]]:translate-y-0.5',
        props.class
      )
    "
  >
    <span
      v-if="label"
      :class="labelled ? 'text-xs text-muted sm:hidden' : 'sr-only sm:hidden'"
      >{{ label }}</span
    >
    <slot />
  </td>
</template>
