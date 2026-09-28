<script setup lang="ts">
import type { MarketChoice } from '~/api/types'
import MarketAvatar from '~/components/MarketAvatar.vue'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '~/components/ui/select'
import { percent } from '~/lib/format'

/** Pick one market of a Polymarket event, with its thumbnail and last YES price. Attributes go to the trigger. */
defineOptions({ inheritAttrs: false })
defineProps<{ choices: MarketChoice[] }>()
const model = defineModel<string>()
</script>

<template>
  <Select v-model="model">
    <SelectTrigger
      v-bind="$attrs"
      class="font-normal"
    >
      <SelectValue placeholder="Pick a market" />
    </SelectTrigger>
    <SelectContent class="max-h-80">
      <SelectItem
        v-for="c in choices"
        :key="c.slug"
        :value="c.slug"
      >
        <span class="flex min-w-0 items-center gap-2">
          <MarketAvatar
            :src="c.image"
            :name="c.label"
            class="size-5 rounded text-[0.625rem]"
          />
          <span class="truncate">{{ c.label }}</span>
          <span
            v-if="c.price != null"
            class="shrink-0 text-xs tabular-nums opacity-70"
            >{{ percent(c.price) }}</span
          >
        </span>
      </SelectItem>
    </SelectContent>
  </Select>
</template>
