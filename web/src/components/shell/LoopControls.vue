<script setup lang="ts">
import { useStatus } from '~/api/queries'
import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import { SimpleTooltip } from '~/components/ui/tooltip'
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
    <SimpleTooltip
      v-if="paused"
      tooltip="Resume: start research rounds and trading again"
      as-child
    >
      <Button
        variant="ghost"
        size="icon"
        :loading="busy === 'resume'"
        aria-label="Resume the loop"
        @click="resume"
      >
        <Icon name="lucide:play" />
      </Button>
    </SimpleTooltip>
    <SimpleTooltip
      v-else
      tooltip="Pause: stop new research rounds and orders. Prices are still checked."
      as-child
    >
      <Button
        variant="ghost"
        size="icon"
        :loading="busy === 'pause'"
        :disabled="status.state === 'stopping'"
        aria-label="Pause the loop"
        @click="pause"
      >
        <Icon name="lucide:pause" />
      </Button>
    </SimpleTooltip>
    <SimpleTooltip
      tooltip="Run a research round now"
      as-child
    >
      <Button
        variant="ghost"
        size="icon"
        :loading="busy === 'pass'"
        :disabled="running"
        aria-label="Run a pass now"
        @click="runPass"
      >
        <Icon name="lucide:refresh-cw" />
      </Button>
    </SimpleTooltip>
  </div>
</template>
