<script setup lang="ts">
import { useMediaQuery } from '@vueuse/core'
import { Toaster } from 'vue-sonner'

import { useLive } from '~/api/live'
import LiveConfirmDialog from '~/components/LiveConfirmDialog.vue'
import AppHeader from '~/components/shell/AppHeader.vue'
import AppSidebar from '~/components/shell/AppSidebar.vue'
import AppTabBar from '~/components/shell/AppTabBar.vue'
import { useColorMode } from '~/composables/useColorMode'
import { useShortcuts } from '~/composables/useShortcuts'

useColorMode()
useLive()
useShortcuts()

const route = useRoute()
const router = useRouter()
const wide = useMediaQuery('(min-width: 1024px)')

/** Keyed on the page, not its params: opening a drawer must not remount the list under it. */
const pageKey = computed(() => String(route.name ?? route.path))

let navigated = false
router.afterEach((to, from) => {
  if (!navigated || to.name === from.name) {
    navigated = true
    return
  }
  void nextTick(() => document.getElementById('main-content')?.focus({ preventScroll: true }))
})
</script>

<template>
  <div class="flex min-h-svh">
    <a
      href="#main-content"
      class="sr-only focus:not-sr-only focus:fixed focus:top-3 focus:left-3 focus:z-50 focus:rounded-lg focus:bg-popover focus:px-3 focus:py-2 focus:text-sm focus:font-semibold focus:text-primary focus:shadow-soft-lg"
    >
      Skip to content
    </a>
    <AppSidebar class="sticky top-0 h-svh" />
    <div class="flex min-w-0 flex-1 flex-col">
      <AppHeader />
      <main
        id="main-content"
        tabindex="-1"
        class="min-w-0 flex-1 overflow-x-clip px-shell-gutter-x py-shell-gutter pb-[calc(var(--shell-gutter)+var(--shell-tabbar-height)+env(safe-area-inset-bottom))] focus:outline-none lg:pb-shell-gutter"
      >
        <RouterView v-slot="{ Component }">
          <div
            :key="pageKey"
            class="page-enter"
          >
            <component :is="Component" />
          </div>
        </RouterView>
      </main>
    </div>
    <AppTabBar />
  </div>
  <LiveConfirmDialog />
  <Toaster
    :position="wide ? 'bottom-right' : 'top-center'"
    :duration="4000"
    :visible-toasts="3"
    close-button
    rich-colors
  />
</template>
