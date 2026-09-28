<script setup lang="ts">
import { useStatus } from '~/api/queries'
import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import { useLoopActions } from '~/composables/useLoopActions'

const { data: status } = useStatus()
const { busy, runPass, pause, resume } = useLoopActions()

const running = computed(() => status.value?.state === 'running' || status.value?.state === 'stopping')
const paused = computed(() => status.value?.state === 'paused')
</script>

<template>
  <div
    v-if="status"
    class="flex items-center gap-1"
  >
    <Button
      v-if="paused"
      variant="ghost"
      size="sm"
      :loading="busy === 'resume'"
      aria-label="Resume the loop"
      @click="resume"
    >
      <Icon name="lucide:play" />
      <span class="hidden @xl:inline">Resume</span>
    </Button>
    <Button
      v-else
      variant="ghost"
      size="sm"
      :loading="busy === 'pause'"
      :disabled="status.state === 'stopping'"
      aria-label="Pause the loop"
      @click="pause"
    >
      <Icon name="lucide:pause" />
      <span class="hidden @xl:inline">Pause</span>
    </Button>
    <Button
      variant="outline"
      size="sm"
      :loading="busy === 'pass'"
      :disabled="running"
      aria-label="Run a pass now"
      @click="runPass"
    >
      <Icon name="lucide:refresh-cw" />
      <span class="hidden @xl:inline">Run pass</span>
    </Button>
  </div>
</template>
