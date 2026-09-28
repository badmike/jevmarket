<script setup lang="ts">
import { toast } from 'vue-sonner'

import { api, errorMessage } from '~/api/client'
import { useRecommendation } from '~/api/queries'
import BriefView from '~/components/BriefView.vue'
import { verdictSentence } from '~/components/MarketVerdict.vue'
import Icon from '~/components/Icon.vue'
import MarketAvatar from '~/components/MarketAvatar.vue'
import OrderDialog from '~/components/OrderDialog.vue'
import OrderStatus from '~/components/OrderStatus.vue'
import ProbabilityBar from '~/components/ProbabilityBar.vue'
import QueryError from '~/components/QueryError.vue'
import VerdictBadge from '~/components/VerdictBadge.vue'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Sheet, SheetContent, SheetDescription, SheetTitle } from '~/components/ui/sheet'
import { Skeleton } from '~/components/ui/skeleton'
import { SimpleTooltip } from '~/components/ui/tooltip'
import { useReadState } from '~/composables/useReadState'
import { useThresholds } from '~/composables/useSettings'
import { at, day, inDays, points, prob, usd } from '~/lib/format'

const props = defineProps<{ slug: string | null }>()
const emit = defineEmits<{ close: [] }>()

const { data, error, isPending, refetch } = useRecommendation(() => props.slug)
const limits = useThresholds()
const busy = ref<'decide' | 'brief' | 'watch' | null>(null)
const { markRead } = useReadState('markets')

/** Opening a market marks its decision read, and so does a newer one arriving while it is open. */
watch(
  [() => data.value?.slug, () => data.value?.ts],
  ([key, ts]) => {
    if (key && ts != null) markRead([{ key, ts }])
  },
  { immediate: true }
)

/** Focus the sheet itself on open, not its first link (the Polymarket page). */
const focusSheet = (event: Event) => {
  event.preventDefault()
  if (event.target instanceof HTMLElement) event.target.focus()
}

const act = async (kind: 'decide' | 'brief' | 'watch') => {
  if (!props.slug) return
  busy.value = kind
  try {
    if (kind === 'decide') {
      await api.decide(props.slug)
      toast.success('Decided again')
    } else if (kind === 'watch') {
      if (data.value?.pinned) {
        await api.unwatch(props.slug)
        toast.success('Taken off the watchlist')
      } else {
        await api.watch(props.slug)
        toast.success('On the watchlist. The price watch checks it every minute.')
      }
    } else {
      await api.refreshBrief(props.slug)
      toast.success('Brief refreshed. Decide again to price it.')
    }
    await refetch()
  } catch (e) {
    toast.error(errorMessage(e))
  } finally {
    busy.value = null
  }
}
</script>

<template>
  <Sheet
    :open="slug !== null"
    @update:open="(open) => !open && emit('close')"
  >
    <SheetContent
      side="right"
      @open-auto-focus="focusSheet"
      class="flex w-full flex-col gap-0 overflow-y-auto p-0 sm:max-w-2xl"
    >
      <div class="sticky top-0 z-10 grid gap-2 border-b border-border bg-background/95 p-4 pr-12 backdrop-blur-md">
        <div class="flex items-center gap-3">
          <MarketAvatar
            v-if="data"
            :src="data.image"
            :name="data.question || data.slug"
            size="md"
          />
          <SheetTitle class="text-lg leading-snug text-primary">
            {{ data?.question ?? slug }}
          </SheetTitle>
        </div>
        <SheetDescription class="flex flex-wrap items-center gap-x-3 gap-y-1">
          <a
            v-if="slug"
            :href="`https://polymarket.com/market/${slug}`"
            target="_blank"
            rel="noopener noreferrer"
            class="inline-flex items-center gap-1 text-accent hover:underline"
          >
            {{ slug }}
            <Icon
              name="lucide:external-link"
              size="12"
              aria-hidden="true"
            />
          </a>
          <span v-if="data">Decided {{ at(data.ts) }}</span>
          <SimpleTooltip
            v-if="data?.end_date"
            :tooltip="`The market is scheduled to settle ${inDays(data.end_date)}. It pays out once the result is confirmed.`"
            as-child
          >
            <span class="underline decoration-muted/50 decoration-dotted underline-offset-4">
              Resolves {{ day(data.end_date) }}
            </span>
          </SimpleTooltip>
        </SheetDescription>
        <div class="flex flex-wrap gap-2 pt-1">
          <Button
            size="sm"
            variant="outline"
            :loading="busy === 'decide'"
            :disabled="busy !== null"
            @click="act('decide')"
          >
            <Icon name="lucide:rotate-ccw" />
            Decide again
          </Button>
          <Button
            size="sm"
            variant="outline"
            :loading="busy === 'brief'"
            :disabled="busy !== null"
            @click="act('brief')"
          >
            <Icon name="lucide:newspaper" />
            Refresh brief
          </Button>
          <SimpleTooltip
            v-if="data && !data.settled"
            :tooltip="
              data.pinned
                ? 'Stop checking this market between research rounds once its view gets old.'
                : 'Check this market\'s price every minute and buy when it drops to the buy line, however old Jev\'s view is.'
            "
            as-child
          >
            <Button
              size="sm"
              variant="outline"
              :loading="busy === 'watch'"
              :disabled="busy !== null"
              @click="act('watch')"
            >
              <Icon :name="data.pinned ? 'lucide:eye-off' : 'lucide:eye'" />
              {{ data.pinned ? 'Unwatch' : 'Watch' }}
            </Button>
          </SimpleTooltip>
          <OrderDialog
            v-if="data"
            :recommendation="data"
          />
        </div>
      </div>

      <div class="grid gap-8 p-4">
        <div
          v-if="isPending"
          class="grid gap-3"
        >
          <Skeleton class="h-6 w-1/2" />
          <Skeleton class="h-24" />
          <Skeleton class="h-48" />
        </div>
        <QueryError
          v-else-if="error"
          :error="error"
          @retry="refetch()"
        />
        <template v-else-if="data">
          <section class="grid gap-3">
            <p class="text-base text-primary">{{ verdictSentence(data, limits) }}</p>
            <div class="flex flex-wrap items-center gap-2">
              <VerdictBadge :action="data.action" />
              <p class="text-xs break-words text-muted">{{ data.reason }}</p>
            </div>
            <div
              v-if="data.trade"
              class="flex flex-wrap items-center gap-2 rounded-lg bg-muted-background/60 p-3 text-sm"
            >
              <OrderStatus
                v-if="data.trade.status"
                :status="data.trade.status"
                :dry-run="data.trade.dry_run"
              />
              <span class="font-semibold text-primary tabular-nums">
                {{ data.trade.side }} {{ data.trade.outcome }} {{ data.trade.size }} @ {{ prob(data.trade.price, 3) }}
              </span>
              <span class="text-muted tabular-nums">{{ usd(data.trade.usd) }}</span>
            </div>
          </section>

          <section class="grid gap-3">
            <h3 class="text-sm font-semibold text-primary">Jev's view</h3>
            <p
              v-if="data.p_yes == null"
              class="text-sm text-muted"
            >
              Only the clarity pre-screen ran: the market was too unclear to pay for research.
            </p>
            <div
              v-else
              class="grid gap-2.5"
            >
              <ProbabilityBar
                label="Jev on YES"
                :value="data.p_yes"
                :series="1"
              />
              <ProbabilityBar
                label="Market price"
                :value="data.midpoint"
                :series="2"
              />
              <ProbabilityBar
                label="Answerable"
                :value="data.answerable"
                :threshold="limits.minAnswerable"
              />
            </div>
            <dl class="grid grid-cols-2 gap-3 text-sm sm:grid-cols-4">
              <div>
                <dt class="text-xs text-muted">
                  <SimpleTooltip
                    tooltip="How much better Jev's estimate is than the price, on the better side, YES or NO."
                    as-child
                  >
                    <span class="underline decoration-muted/50 decoration-dotted underline-offset-4">Best edge</span>
                  </SimpleTooltip>
                </dt>
                <dd
                  class="font-semibold tabular-nums"
                  :class="data.action.startsWith('trade') ? 'text-success' : 'text-primary'"
                >
                  {{ points(data.edge) }} <span class="font-normal text-muted">{{ data.side }}</span>
                </dd>
              </div>
              <div>
                <dt class="text-xs text-muted">
                  <SimpleTooltip
                    tooltip="How clear the market's rules are about what counts as YES, from 0 (vague) to 4 (precise)."
                    as-child
                  >
                    <span class="underline decoration-muted/50 decoration-dotted underline-offset-4">Clarity</span>
                  </SimpleTooltip>
                </dt>
                <dd class="font-semibold text-primary tabular-nums">
                  {{ data.clarity ?? '–' }} of 4
                  <span class="font-normal text-muted">(needs {{ limits.minClarity }})</span>
                </dd>
              </div>
              <div>
                <dt class="text-xs text-muted">
                  <SimpleTooltip
                    tooltip="The cheapest price someone is selling YES and NO shares for right now. A share pays $1 if it wins."
                    as-child
                  >
                    <span class="underline decoration-muted/50 decoration-dotted underline-offset-4">Asks YES / NO</span>
                  </SimpleTooltip>
                </dt>
                <dd class="font-semibold text-primary tabular-nums">
                  {{ prob(data.yes_ask) }} / {{ prob(data.no_ask) }}
                </dd>
              </div>
              <div>
                <dt class="text-xs text-muted">
                  <SimpleTooltip
                    tooltip="What asking Jev and researching the news cost on OpenRouter."
                    as-child
                  >
                    <span class="underline decoration-muted/50 decoration-dotted underline-offset-4">Cost</span>
                  </SimpleTooltip>
                </dt>
                <dd class="font-semibold text-primary tabular-nums">
                  {{ usd(data.jev_cost + (data.research_cost ?? 0), true) }}
                </dd>
              </div>
            </dl>
          </section>

          <section class="grid gap-3">
            <h3 class="text-sm font-semibold text-primary">Evidence brief</h3>
            <BriefView
              v-if="data.brief"
              :record="data.brief"
              :max-move="limits.maxPriceMove"
            />
            <EmptyState
              v-else
              icon="lucide:file-question"
              title="No brief"
              description="Research was off, over its budget for the pass, or the pre-screen skipped this market."
            />
          </section>

          <details class="group rounded-lg border border-border">
            <summary class="flex cursor-pointer items-center gap-2 p-3 text-sm font-semibold text-primary">
              <Icon
                name="lucide:chevron-right"
                class="transition-transform group-open:rotate-90"
                aria-hidden="true"
              />
              State sent to Jev
            </summary>
            <pre class="overflow-x-auto border-t border-border p-3 text-xs">{{ JSON.stringify(data.state, null, 2) }}</pre>
          </details>
        </template>
      </div>
    </SheetContent>
  </Sheet>
</template>
