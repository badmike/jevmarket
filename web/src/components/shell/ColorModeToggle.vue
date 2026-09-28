<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import { SimpleTooltip } from '~/components/ui/tooltip'
import { type ColorMode, useColorMode } from '~/composables/useColorMode'

const { mode, setMode } = useColorMode()

const next: Record<ColorMode, ColorMode> = { system: 'light', light: 'dark', dark: 'system' }
const icons: Record<ColorMode, string> = {
  light: 'lucide:sun',
  dark: 'lucide:moon',
  system: 'lucide:monitor',
}
const labels: Record<ColorMode, string> = { light: 'Light', dark: 'Dark', system: 'System' }
</script>

<template>
  <SimpleTooltip
    :tooltip="`Theme: ${labels[mode]}. Switch to ${labels[next[mode]].toLowerCase()}.`"
    as-child
  >
    <Button
      variant="ghost"
      size="icon"
      :aria-label="`Theme: ${labels[mode]}`"
      @click="setMode(next[mode])"
    >
      <Icon :name="icons[mode]" />
    </Button>
  </SimpleTooltip>
</template>
