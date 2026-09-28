<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import { navigation } from '~/router'

/** The phone's navigation, hidden from `lg` up where the sidebar takes over. */
</script>

<template>
  <nav
    aria-label="Main"
    class="fixed inset-x-0 bottom-0 z-30 border-t border-border bg-background/85 pb-safe backdrop-blur-md lg:hidden"
  >
    <ul class="grid h-shell-tabbar auto-cols-fr grid-flow-col">
      <li
        v-for="item in navigation"
        :key="item.name"
      >
        <RouterLink
          :to="{ name: item.name }"
          class="group/tab flex h-full flex-col items-center justify-center gap-1 text-2xs font-medium text-muted transition-colors duration-200 ease-butter active:text-primary"
          active-class="text-primary"
        >
          <span
            class="grid h-7 w-11 place-items-center rounded-full transition-colors duration-200 ease-butter group-[.router-link-active]/tab:bg-secondary"
          >
            <Icon
              :name="item.icon"
              size="18"
              aria-hidden="true"
            />
          </span>
          <span class="truncate">{{ item.label }}</span>
        </RouterLink>
      </li>
    </ul>
  </nav>
</template>
