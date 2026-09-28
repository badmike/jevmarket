<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import type { navigation } from '~/router'

defineProps<{ item: (typeof navigation)[number] }>()
</script>

<template>
  <RouterLink
    :to="{ name: item.name }"
    class="group/nav flex h-shell-row w-full items-center gap-2.5 rounded-lg px-2 text-sm text-sidebar-muted transition-colors duration-200 ease-butter hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
    active-class="bg-sidebar-accent font-medium text-sidebar-accent-foreground!"
  >
    <Icon
      :name="item.icon"
      size="16"
      class="shrink-0 opacity-80 group-hover/nav:opacity-100"
      aria-hidden="true"
    />
    <span class="flex-1 truncate">{{ item.label }}</span>
    <kbd
      class="hidden rounded border border-sidebar-border px-1 text-2xs text-sidebar-muted opacity-0 transition-opacity group-hover/nav:opacity-100 xl:inline"
      aria-hidden="true"
    >
      g {{ item.key }}
    </kbd>
  </RouterLink>
</template>
