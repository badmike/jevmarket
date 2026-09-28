<script setup lang="ts">
import type { HTMLAttributes } from 'vue'

import { cn } from '~/lib/utils'

/**
 * A list in its own box. The table draws one border and one radius around
 * itself; it is never put inside a `Card`, because a card around a bordered
 * table is a box inside a box. The border is what separates the list from the
 * page, and it is what the header's fill sits against.
 *
 * The box sits on the page's content edge, flush with the toolbar above it and
 * the pager below it. It used to bleed into the gutter by a cell's padding so
 * that the first column's text lined up with the page heading; once the table
 * drew a border, that bleed put its edge outside everything around it. A
 * column of text starting 12px in is the cheaper defect.
 *
 * `page` is a table that owns its width: its rows stack into records on a
 * phone. `boxed` is a table inside something else — a panel, a field editor —
 * where that is not wanted.
 */
const props = withDefaults(
  defineProps<{
    class?: HTMLAttributes['class']
    /** The accessible name. A table without one is "table" to a screen reader. */
    label?: string
    variant?: 'page' | 'boxed'
  }>(),
  {
    class: undefined,
    label: undefined,
    variant: 'page',
  }
)
</script>

<template>
  <div class="overflow-hidden rounded-lg border border-border bg-card">
    <div class="scroll-fade-x relative w-full">
      <table
        role="table"
        :aria-label="label"
        :class="
          cn('w-full caption-bottom text-sm', variant === 'page' && 'table-stacked', props.class)
        "
      >
        <slot />
      </table>
    </div>
  </div>
</template>
