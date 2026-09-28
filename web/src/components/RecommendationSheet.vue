<script setup lang="ts">
import { toast } from 'vue-sonner'

import { api, errorMessage } from '~/api/client'
import { useRecommendation } from '~/api/queries'
import BriefView from '~/components/BriefView.vue'
import Icon from '~/components/Icon.vue'
import OrderStatus from '~/components/OrderStatus.vue'
import ProbabilityBar from '~/components/ProbabilityBar.vue'
import QueryError from '~/components/QueryError.vue'
import VerdictBadge from '~/components/VerdictBadge.vue'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Sheet, SheetContent, SheetDescription, SheetTitle } from '~/components/ui/sheet'
import { Skeleton } from '~/components/ui/skeleton'
import { useThresholds } from '~/composables/useSettings'
import { at, points, prob, usd } from '~/lib/format'

const props = defineProps<{ slug: string | null }>()
const emit = defineEmits<{ close: [] }>()

const { data, error, isPending, refetch } = useRecommendation(() => props.slug)
const limits = useThresholds()
const busy = ref<'decide' | 'brief' | null>(null)

/** Focus the sheet itself on open, not its first link (the Polymarket page). */
const focusSheet = (event: Event) => {
  event.preventDefault()
  if (event.target instanceof HTMLElement) event.target.focus()
}

const act = async (kind: 'decide' | 'brief') => {
  if (!props.slug) return
  busy.value = kind
  try {
    if (kind === 'decide') {
      await api.decide(props.slug)
      toast.success('Decided again')
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
        <SheetTitle class="text-lg leading-snug text-primary">
          {{ data?.question ?? slug }}
        </SheetTitle>
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
            <div class="flex flex-wrap items-center gap-2">
              <VerdictBadge :action="data.action" />
              <p class="text-sm break-words">{{ data.reason }}</p>
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
                BUY {{ data.trade.outcome }} {{ data.trade.size }} @ {{ prob(data.trade.price, 3) }}
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
                label="Jev P(YES)"
                :value="data.p_yes"
                :series="1"
              />
              <ProbabilityBar
                label="Market midpoint"
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
                <dt class="text-xs text-muted">Best edge</dt>
                <dd
                  class="font-semibold tabular-nums"
                  :class="data.action.startsWith('trade') ? 'text-success' : 'text-primary'"
                >
                  {{ points(data.edge) }} <span class="font-normal text-muted">{{ data.side }}</span>
                </dd>
              </div>
              <div>
                <dt class="text-xs text-muted">Clarity</dt>
                <dd class="font-semibold text-primary tabular-nums">
                  {{ data.clarity ?? '–' }} of 4
                  <span class="font-normal text-muted">(needs {{ limits.minClarity }})</span>
                </dd>
              </div>
              <div>
                <dt class="text-xs text-muted">Asks YES / NO</dt>
                <dd class="font-semibold text-primary tabular-nums">
                  {{ prob(data.yes_ask) }} / {{ prob(data.no_ask) }}
                </dd>
              </div>
              <div>
                <dt class="text-xs text-muted">Cost</dt>
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
