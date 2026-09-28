<script setup lang="ts">
import { toast } from 'vue-sonner'

import { api, errorMessage } from '~/api/client'
import Icon from '~/components/Icon.vue'
import MarketChoiceSelect from '~/components/MarketChoiceSelect.vue'
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
import { useMarketReference } from '~/composables/useMarketReference'

const router = useRouter()
const open = ref(false)
const { reference, choices, choice, target, offerChoices } = useMarketReference()
const fresh = ref(false)
const busy = ref(false)
const error = ref<string | null>(null)

const submit = async () => {
  if (!target.value || busy.value) return
  busy.value = true
  error.value = null
  try {
    const r = await api.decide(target.value, fresh.value)
    toast.success(`Decided: ${r.question}`)
    open.value = false
    reference.value = ''
    await router.push({ name: 'markets', params: { slug: r.slug } })
  } catch (e) {
    if (!offerChoices(e)) error.value = errorMessage(e)
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
        <div
          v-if="choices.length"
          class="grid gap-1.5"
        >
          <Label for="decide-choice">This event has {{ choices.length }} open markets. Which one?</Label>
          <MarketChoiceSelect
            id="decide-choice"
            v-model="choice"
            :choices="choices"
          />
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
            :disabled="!target || (choices.length > 0 && !choice)"
          >
            Decide
          </Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
</template>
