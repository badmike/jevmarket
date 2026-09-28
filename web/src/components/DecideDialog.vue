<script setup lang="ts">
import { toast } from 'vue-sonner'

import { api, errorMessage } from '~/api/client'
import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '~/components/ui/dialog'
import { Input } from '~/components/ui/input'
import { Label } from '~/components/ui/label'
import { Switch } from '~/components/ui/switch'

const router = useRouter()
const open = ref(false)
const reference = ref('')
const fresh = ref(false)
const busy = ref(false)
const error = ref<string | null>(null)

const submit = async () => {
  if (!reference.value.trim()) return
  busy.value = true
  error.value = null
  try {
    const r = await api.decide(reference.value.trim(), fresh.value)
    toast.success(`Decided: ${r.question}`)
    open.value = false
    reference.value = ''
    await router.push({ name: 'markets', params: { slug: r.slug } })
  } catch (e) {
    error.value = errorMessage(e)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogTrigger as-child>
      <Button variant="accent">
        <Icon name="lucide:scan-search" />
        Decide a market
      </Button>
    </DialogTrigger>
    <DialogContent
      submit-shortcut
      @submit="submit"
    >
      <form
        class="grid gap-4"
        @submit.prevent="submit"
      >
        <DialogHeader>
          <DialogTitle>Decide a market</DialogTitle>
          <DialogDescription>
            Research it, ask Jev and log the decision. This never places an order. Research can take a minute.
          </DialogDescription>
        </DialogHeader>
        <div class="grid gap-1.5">
          <Label for="decide-ref">Market slug or polymarket.com URL</Label>
          <Input
            id="decide-ref"
            v-model="reference"
            placeholder="https://polymarket.com/event/…"
            autocomplete="off"
            :aria-invalid="!!error"
            aria-describedby="decide-error"
            autofocus
          />
          <p
            v-if="error"
            id="decide-error"
            class="text-sm text-destructive"
          >
            {{ error }}
          </p>
        </div>
        <div class="flex items-center gap-2">
          <Switch
            id="decide-fresh"
            v-model="fresh"
          />
          <Label for="decide-fresh">Research again, ignore the cached brief</Label>
        </div>
        <DialogFooter>
          <Button
            type="submit"
            variant="accent"
            :loading="busy"
            :disabled="!reference.trim()"
          >
            Decide
          </Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
</template>
