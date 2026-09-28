<script setup lang="ts">
import { useStatus } from '~/api/queries'
import AppBrand from '~/components/shell/AppBrand.vue'
import NavLink from '~/components/shell/NavLink.vue'
import { navigation } from '~/router'

const { data: status } = useStatus()
</script>

<template>
  <aside
    class="relative hidden w-56 shrink-0 flex-col border-r border-sidebar-border bg-sidebar text-sidebar-foreground select-none lg:flex"
  >
    <div class="flex h-shell-header shrink-0 items-center px-2">
      <AppBrand />
    </div>
    <nav
      aria-label="Main"
      class="flex min-h-0 flex-1 flex-col gap-shell-gap overflow-y-auto px-2 py-2"
    >
      <NavLink
        v-for="item in navigation"
        :key="item.name"
        :item="item"
      />
    </nav>
    <p
      v-if="status"
      class="shrink-0 truncate px-4 pb-3 text-2xs text-sidebar-muted"
    >
      jevmarket {{ status.version }}
    </p>
  </aside>
</template>
