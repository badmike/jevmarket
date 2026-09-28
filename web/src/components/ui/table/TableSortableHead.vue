<script setup lang="ts">
import { computed } from 'vue'

import Icon from '~/components/Icon.vue'
import type { TableSort } from '~/lib/table'

import TableHead from './TableHead.vue'

/**
 * A column header that sorts. It is a button, not a clickable `th`: sorting a
 * table from the keyboard is the same gesture as sorting it with the mouse.
 *
 * The arrow is solid on the sorted column and faint on every other sortable
 * one, so which columns can be sorted is visible before anything is hovered.
 */
const sortBy = defineModel<TableSort>({ required: true })

const props = withDefaults(
  defineProps<{
    column: string
    sortable?: boolean
    /** Right-align the label, for a numeric column. */
    align?: 'start' | 'end'
  }>(),
  {
    sortable: true,
    align: 'start',
  }
)

const isActive = computed(() => sortBy.value.column === props.column)

const ariaSort = computed(() => {
  if (!props.sortable || !isActive.value) return 'none'

  return sortBy.value.direction === 'asc' ? 'ascending' : 'descending'
})

const toggle = () => {
  if (!props.sortable) return

  sortBy.value = isActive.value
    ? { column: props.column, direction: sortBy.value.direction === 'asc' ? 'desc' : 'asc' }
    : { column: props.column, direction: 'asc' }
}
</script>

<template>
  <TableHead :aria-sort="ariaSort">
    <button
      v-if="sortable"
      type="button"
      class="group -mx-1 flex w-full cursor-pointer items-center gap-1 rounded px-1 py-0.5 text-inherit transition-colors hover:text-primary focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
      :class="align === 'end' ? 'justify-end' : 'justify-start'"
      @click="toggle"
    >
      <slot />
      <Icon
        :name="isActive && sortBy.direction === 'desc' ? 'lucide:arrow-down' : 'lucide:arrow-up'"
        size="12"
        class="shrink-0 transition-opacity"
        :class="
          isActive
            ? 'opacity-100'
            : 'opacity-35 group-hover:opacity-80 group-focus-visible:opacity-80'
        "
        aria-hidden="true"
      />
    </button>
    <slot v-else />
  </TableHead>
</template>
