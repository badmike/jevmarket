<script setup lang="ts">
import { toast } from 'vue-sonner'

import { api, errorMessage } from '~/api/client'
import { useStatus } from '~/api/queries'
import type { Recommendation, Side } from '~/api/types'
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
import { useLiveConfirm } from '~/composables/useLiveConfirm'
import { useThresholds } from '~/composables/useSettings'
import { prob, usd } from '~/lib/format'

const props = defineProps<{ recommendation: Recommendation }>()
const emit = defineEmits<{ placed: [] }>()

const { data: status } = useStatus()
const limits = useThresholds()
const { guarded } = useLiveConfirm()

const open = ref(false)
const outcome = ref<Side>('YES')
const price = ref<string | number>('')
const amount = ref<string | number>('')
const busy = ref(false)
const error = ref<string | null>(null)

const ask = (side: Side) =>
  side === 'YES' ? props.recommendation.yes_ask : props.recommendation.no_ask

/** Start from the signal's trade, else Jev's side at its ask, sized at the per-trade cap. */
watch(open, (isOpen) => {
  if (!isOpen) return
  const r = props.recommendation
  const side = r.trade?.outcome === 'NO' || (!r.trade && r.side === 'NO') ? 'NO' : 'YES'
  outcome.value = side
  price.value = r.trade?.price ?? ask(side) ?? ''
  amount.value = Math.round((r.trade?.usd ?? limits.value.maxUsdPerTrade) * 100) / 100
  error.value = null
})

const pick = (side: Side) => {
  outcome.value = side
  price.value = ask(side) ?? price.value
}

const priceValue = computed(() => Number(price.value))
const usdValue = computed(() => Number(amount.value))
const valid = computed(
  () => priceValue.value > 0 && priceValue.value < 1 && usdValue.value > 0
)
const shares = computed(() =>
  valid.value ? Math.floor((usdValue.value / priceValue.value) * 100) / 100 : null
)
const live = computed(() => status.value?.dry_run === false)

const submit = async () => {
  if (!valid.value || busy.value) return
  busy.value = true
  error.value = null
  const order = {
    reference: props.recommendation.slug,
    outcome: outcome.value,
    price: priceValue.value,
    usd: usdValue.value,
  }
  try {
    const placed = await guarded(
      'Place a real order?',
      `BUY ${order.outcome} at ${prob(order.price, 3)} for about ${usd(order.usd)} on Polymarket.`,
      (confirm) => api.placeOrder(order, confirm)
    )
    if (!placed) return
    toast.success(
      `${placed.dry_run ? 'Dry-run order logged' : `Order ${placed.status}`}: BUY ${placed.outcome} ${placed.size} @ ${prob(placed.price, 3)}`
    )
    open.value = false
    emit('placed')
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
      >
        <Icon name="lucide:shopping-cart" />
        Order
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
          <DialogTitle>Manual order</DialogTitle>
          <DialogDescription>
            A limit BUY on the current book, whatever the signal said. The per-trade and exposure caps
            still apply.
            <template v-if="live">It goes to Polymarket with real money.</template>
            <template v-else>Dry run: it is logged, not sent.</template>
          </DialogDescription>
        </DialogHeader>
        <div class="grid gap-1.5">
          <Label>Outcome</Label>
          <div class="grid grid-cols-2 gap-2">
            <Button
              v-for="side in ['YES', 'NO'] as const"
              :key="side"
              type="button"
              :variant="outcome === side ? 'primary' : 'outline'"
              :aria-pressed="outcome === side"
              @click="pick(side)"
            >
              {{ side }}
              <span class="font-normal tabular-nums opacity-70">ask {{ prob(ask(side)) }}</span>
            </Button>
          </div>
        </div>
        <div class="grid grid-cols-2 gap-3">
          <div class="grid gap-1.5">
            <Label for="order-price">Limit price</Label>
            <Input
              id="order-price"
              v-model="price"
              type="number"
              inputmode="decimal"
              min="0.001"
              max="0.999"
              step="any"
            />
          </div>
          <div class="grid gap-1.5">
            <Label for="order-usd">Amount (USD)</Label>
            <Input
              id="order-usd"
              v-model="amount"
              type="number"
              inputmode="decimal"
              min="0.01"
              step="any"
            />
          </div>
        </div>
        <p class="text-sm text-muted tabular-nums">
          <template v-if="shares !== null">About {{ shares }} shares. The daemon rounds to the tick and minimum size.</template>
          <template v-else>Enter a price between 0 and 1 and a positive amount.</template>
        </p>
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
            {{ live ? 'Place order' : 'Log dry-run order' }}
          </Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
</template>
