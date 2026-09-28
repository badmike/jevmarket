<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '~/components/ui/select'
import type { TableSort } from '~/lib/table'

/** A table's sort as a menu, for the phone layout where the column headers are out of sight. */
defineProps<{ options: { column: string; label: string }[] }>()
const sort = defineModel<TableSort>({ required: true })

const column = computed({
  get: () => sort.value.column,
  set: (column: string) => (sort.value = { ...sort.value, column }),
})
const ascending = computed(() => sort.value.direction === 'asc')
const flip = () => (sort.value = { ...sort.value, direction: ascending.value ? 'desc' : 'asc' })
</script>

<template>
  <div class="flex items-center gap-1">
    <Select v-model="column">
      <SelectTrigger
        aria-label="Sort by"
        class="w-auto gap-1.5 font-normal"
      >
        <span class="text-muted">Sort</span>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectItem
          v-for="o in options"
          :key="o.column"
          :value="o.column"
        >
          {{ o.label }}
        </SelectItem>
      </SelectContent>
    </Select>
    <Button
      variant="ghost"
      size="icon"
      :aria-label="ascending ? 'Ascending, switch to descending' : 'Descending, switch to ascending'"
      @click="flip"
    >
      <Icon :name="ascending ? 'lucide:arrow-up' : 'lucide:arrow-down'" />
    </Button>
  </div>
</template>
