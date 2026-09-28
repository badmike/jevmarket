<script setup lang="ts">
import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '~/components/ui/alert-dialog'
import { Button } from '~/components/ui/button'
import { Input } from '~/components/ui/input'
import { Label } from '~/components/ui/label'
import { useLiveConfirm } from '~/composables/useLiveConfirm'

const { current } = useLiveConfirm()
const typed = ref('')
const field = useTemplateRef('field')
const matches = computed(() => typed.value.trim() === current.value?.phrase)

watch(current, () => (typed.value = ''))

/** Reka focuses Cancel on a later tick; the phrase field goes first once that has run. */
const focusField = () => setTimeout(() => field.value?.focus())

const confirm = () => {
  if (matches.value) current.value?.settle(current.value.phrase)
}
</script>

<template>
  <AlertDialog
    :open="current !== null"
    @update:open="(open) => !open && current?.settle(null)"
  >
    <AlertDialogContent
      v-if="current"
      @open-auto-focus="focusField"
    >
      <form
        class="grid gap-4"
        @submit.prevent="confirm"
      >
        <AlertDialogHeader>
          <AlertDialogTitle class="text-primary">{{ current.title }}</AlertDialogTitle>
          <AlertDialogDescription class="text-sm text-muted">
            {{ current.message }} Only continue with money you are willing to lose.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <div class="grid gap-1.5">
          <Label for="live-confirm">Type {{ current.phrase }} to confirm</Label>
          <Input
            id="live-confirm"
            ref="field"
            v-model="typed"
            autocomplete="off"
            autocapitalize="characters"
            spellcheck="false"
            :placeholder="current.phrase"
          />
        </div>
        <AlertDialogFooter>
          <AlertDialogCancel type="button">Cancel</AlertDialogCancel>
          <Button
            type="submit"
            variant="destructive"
            class="mt-2 sm:mt-0"
            :disabled="!matches"
          >
            Place real orders
          </Button>
        </AlertDialogFooter>
      </form>
    </AlertDialogContent>
  </AlertDialog>
</template>
