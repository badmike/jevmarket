<script setup lang="ts">
import AppBrand from '~/components/shell/AppBrand.vue'
import ColorModeToggle from '~/components/shell/ColorModeToggle.vue'
import ConnectionIndicator from '~/components/shell/ConnectionIndicator.vue'
import LoopControls from '~/components/shell/LoopControls.vue'
import ModeBadge from '~/components/shell/ModeBadge.vue'

const route = useRoute()
</script>

<template>
  <header
    class="@container sticky top-0 z-20 flex shrink-0 items-center gap-2 border-b border-border bg-background/85 px-3 pt-safe backdrop-blur-md"
  >
    <div class="flex h-shell-header min-w-0 flex-1 items-center gap-2">
      <AppBrand
        compact
        class="lg:hidden"
      />
      <p class="hidden truncate px-1 text-sm font-medium text-primary @md:block">
        {{ route.meta.title }}
      </p>
      <div class="ml-auto flex items-center gap-1.5">
        <ConnectionIndicator />
        <ModeBadge />
        <LoopControls />
        <ColorModeToggle />
      </div>
    </div>
  </header>
</template>
