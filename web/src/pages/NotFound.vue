<script setup lang="ts">
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
</script>

<template>
  <EmptyState
    icon="lucide:map-pin-off"
    title="Nothing here"
    description="This page does not exist in the console."
    class="mx-auto mt-12 max-w-lg"
  >
    <Button
      as-child
      size="sm"
    >
      <RouterLink :to="{ name: 'overview' }">Go to the overview</RouterLink>
    </Button>
  </EmptyState>
</template>
