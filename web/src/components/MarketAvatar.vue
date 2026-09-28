<script setup lang="ts">
import type { HTMLAttributes } from 'vue'

import { cn } from '~/lib/utils'

/**
 * A market's Polymarket thumbnail. Decorative: the question always sits next to it. Without an
 * image, or when it fails to load, the question's first letter stands in.
 */
const props = withDefaults(
  defineProps<{
    src: string | null | undefined
    name: string
    size?: 'sm' | 'md' | 'lg'
    class?: HTMLAttributes['class']
  }>(),
  { size: 'sm', class: undefined }
)

const failed = ref(false)
watch(
  () => props.src,
  () => (failed.value = false)
)

const sizes = {
  sm: 'size-8 rounded-md text-xs',
  md: 'size-10 rounded-lg text-sm',
  lg: 'size-14 rounded-xl text-lg',
}
</script>

<template>
  <span
    aria-hidden="true"
    :class="
      cn(
        'grid shrink-0 place-items-center overflow-hidden bg-muted-background font-semibold text-muted',
        sizes[size],
        props.class
      )
    "
  >
    <img
      v-if="src && !failed"
      :src="src"
      alt=""
      loading="lazy"
      decoding="async"
      referrerpolicy="no-referrer"
      class="size-full object-cover"
      @error="failed = true"
    />
    <template v-else>{{ name.trim().charAt(0).toUpperCase() }}</template>
  </span>
</template>
