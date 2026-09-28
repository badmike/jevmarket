<script setup lang="ts">
import { toast } from 'vue-sonner'

import { api, errorMessage } from '~/api/client'
import type { WalletKind, WalletState } from '~/api/types'
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
import { usd } from '~/lib/format'

const props = defineProps<{ from: WalletKind; source: WalletState; target: WalletState }>()
const emit = defineEmits<{ moved: [] }>()

const LABELS: Record<WalletKind, string> = { deposit: 'deposit wallet', proxy: 'polymarket.com wallet' }
const toLabel = computed(() => LABELS[props.from === 'deposit' ? 'proxy' : 'deposit'])

const open = ref(false)
const amount = ref<string | number>('')
const busy = ref(false)
const error = ref<string | null>(null)

watch(open, (isOpen) => {
  if (!isOpen) return
  amount.value = ''
  error.value = null
})

const value = computed(() => Number(amount.value))
/** Max sends the exact balance, down to the last base unit. */
const isMax = computed(() => value.value === props.source.balance_usd)
const valid = computed(() => value.value > 0 && value.value <= props.source.balance_usd)

const submit = async () => {
  if (!valid.value || busy.value) return
  busy.value = true
  error.value = null
  try {
    const t = await api.transfer(props.from, isMax.value ? null : value.value)
    toast.success(`Moved ${usd(t.amount_usd)} to the ${toLabel.value}`, {
      action: {
        label: 'Polygonscan',
        onClick: () => window.open(`https://polygonscan.com/tx/${t.tx_hash}`, '_blank', 'noopener'),
      },
    })
    open.value = false
    emit('moved')
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
      <Button
        size="sm"
        variant="outline"
        :disabled="source.balance_usd <= 0"
      >
        <Icon name="lucide:arrow-right-left" />
        Move to {{ toLabel }}
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
          <DialogTitle>Move pUSD to the {{ toLabel }}</DialogTitle>
          <DialogDescription>
            Between your own two wallets, through Polymarket's relayer: no gas, usually done in seconds. Money in
            open positions or orders stays where it is.
          </DialogDescription>
        </DialogHeader>
        <div class="grid gap-1.5">
          <Label for="transfer-amount">Amount (USD)</Label>
          <div class="flex gap-2">
            <Input
              id="transfer-amount"
              v-model="amount"
              type="number"
              inputmode="decimal"
              min="0.01"
              step="any"
              :placeholder="`Up to ${usd(source.balance_usd)}`"
              autofocus
            />
            <Button
              type="button"
              variant="outline"
              @click="amount = source.balance_usd"
            >
              Max
            </Button>
          </div>
          <p class="text-sm text-muted tabular-nums">
            {{ usd(source.balance_usd) }} available. The {{ toLabel }} holds {{ usd(target.balance_usd) }}.
          </p>
        </div>
        <p
          v-if="error"
          class="text-sm break-words text-destructive"
        >
          {{ error }}
        </p>
        <DialogFooter>
          <Button
            type="submit"
            variant="accent"
            :loading="busy"
            :disabled="!valid"
          >
            Move {{ valid ? usd(value) : '' }}
          </Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
</template>
