<script setup lang="ts">
import { errorMessage } from '~/api/client'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'

defineProps<{ error: unknown; title?: string }>()
defineEmits<{ retry: [] }>()
</script>

<template>
  <EmptyState
    icon="lucide:triangle-alert"
    tone="error"
    :title="title ?? 'Could not load this'"
    :description="errorMessage(error)"
  >
    <Button
      size="sm"
      @click="$emit('retry')"
    >
      Try again
    </Button>
  </EmptyState>
</template>
