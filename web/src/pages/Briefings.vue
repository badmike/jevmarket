<script setup lang="ts">
import { useNow } from '@vueuse/core'
import { toast } from 'vue-sonner'

import { api, errorMessage } from '~/api/client'
import { useBrief, useBriefs } from '~/api/queries'
import BriefView from '~/components/BriefView.vue'
import Icon from '~/components/Icon.vue'
import QueryError from '~/components/QueryError.vue'
import { Badge } from '~/components/ui/badge'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Input } from '~/components/ui/input'
import { Skeleton } from '~/components/ui/skeleton'
import { useThresholds } from '~/composables/useSettings'
import { ago } from '~/lib/format'

const route = useRoute()
const router = useRouter()
const list = useBriefs()
const limits = useThresholds()
const now = useNow({ interval: 30_000 })
const search = ref('')

const selectedId = computed(() => {
  const id = Number(route.params.id)
  return Number.isInteger(id) && id > 0 ? id : null
})
const reader = useBrief(selectedId)

const briefs = computed(() => {
  const q = search.value.trim().toLowerCase()
  return (list.data.value ?? []).filter(
    (b) => !q || (b.question ?? '').toLowerCase().includes(q) || b.slug.includes(q) || b.summary.toLowerCase().includes(q)
  )
})

/** A new brief for the open market replaces the one on screen. */
watch(
  () => list.data.value,
  (all) => {
    const current = reader.data.value
    const newer = current && all?.find((b) => b.slug === current.slug && b.id !== current.id && b.ts > current.ts)
    if (newer) void router.replace({ name: 'briefings', params: { id: newer.id } })
  }
)

const refreshing = ref(false)
const refresh = async () => {
  const slug = reader.data.value?.slug
  if (!slug) return
  refreshing.value = true
  try {
    const b = await api.refreshBrief(slug)
    toast.success('Brief refreshed')
    await router.replace({ name: 'briefings', params: { id: b.id } })
  } catch (e) {
    toast.error(errorMessage(e))
  } finally {
    refreshing.value = false
  }
}
</script>

<template>
  <div class="mx-auto grid max-w-7xl gap-4">
    <header class="grid gap-1">
      <h1 class="text-2xl font-semibold text-primary">Briefings</h1>
      <p class="text-sm text-muted">
        The researcher's dated evidence per market. Jev prices from these; it never sees the sources.
      </p>
    </header>

    <QueryError
      v-if="list.error.value"
      :error="list.error.value"
      @retry="list.refetch()"
    />
    <EmptyState
      v-else-if="!list.isPending.value && !list.data.value?.length"
      icon="lucide:newspaper"
      title="No briefs yet"
      description="Passes research the most promising markets first. Briefs appear here as they are written."
    />
    <div
      v-else
      class="grid gap-4 lg:grid-cols-[22rem_1fr]"
    >
      <aside
        class="grid content-start gap-2"
        :class="selectedId !== null && 'max-lg:hidden'"
      >
        <Input
          v-model="search"
          type="search"
          placeholder="Search briefs  /"
          aria-label="Search briefs"
        />
        <div
          v-if="list.isPending.value"
          class="grid gap-2"
        >
          <Skeleton
            v-for="i in 6"
            :key="i"
            class="h-20"
          />
        </div>
        <nav
          v-else
          aria-label="Briefs"
          class="grid gap-1 lg:max-h-[calc(100svh-14rem)] lg:overflow-y-auto lg:pr-1"
        >
          <RouterLink
            v-for="b in briefs"
            :key="b.id"
            :to="{ name: 'briefings', params: { id: b.id } }"
            class="grid gap-1 rounded-lg border border-transparent p-3 transition-colors hover:bg-secondary/60"
            active-class="border-border bg-card shadow-soft-sm"
          >
            <span class="line-clamp-2 text-sm font-medium text-primary">{{ b.question ?? b.slug }}</span>
            <span class="flex flex-wrap items-center gap-2 text-xs text-muted">
              <Badge
                :variant="b.fresh ? 'success' : 'warning'"
                size="sm"
              >
                {{ b.fresh ? 'Fresh' : 'Expired' }}
              </Badge>
              <span>as of {{ b.as_of || '?' }}</span>
              <span>{{ ago(b.ts, now.getTime() / 1000) }}</span>
              <span>{{ b.facts }} facts, {{ b.sources }} sources</span>
            </span>
          </RouterLink>
          <p
            v-if="!briefs.length"
            class="p-3 text-sm text-muted"
          >
            No brief matches.
          </p>
        </nav>
      </aside>

      <section
        class="min-w-0 rounded-xl bg-card p-4 shadow-soft sm:p-6"
        :class="selectedId === null && 'max-lg:hidden'"
        aria-live="polite"
      >
        <EmptyState
          v-if="selectedId === null"
          icon="lucide:book-open"
          title="Pick a brief"
          description="Select a market on the left to read its evidence."
          class="border-none"
        />
        <div
          v-else-if="reader.isPending.value"
          class="grid gap-3"
        >
          <Skeleton class="h-8 w-2/3" />
          <Skeleton class="h-24" />
          <Skeleton class="h-64" />
        </div>
        <QueryError
          v-else-if="reader.error.value"
          :error="reader.error.value"
          @retry="reader.refetch()"
        />
        <div
          v-else-if="reader.data.value"
          class="grid gap-6"
        >
          <div class="flex flex-wrap items-start justify-between gap-3">
            <div class="grid min-w-0 gap-1">
              <RouterLink
                :to="{ name: 'briefings' }"
                class="inline-flex items-center gap-1 text-sm text-muted hover:text-primary lg:hidden"
              >
                <Icon
                  name="lucide:arrow-left"
                  aria-hidden="true"
                />
                All briefs
              </RouterLink>
              <h2 class="text-xl font-semibold text-primary">
                {{ reader.data.value.question ?? reader.data.value.slug }}
              </h2>
              <RouterLink
                :to="{ name: 'markets', params: { slug: reader.data.value.slug } }"
                class="text-sm text-accent hover:underline"
              >
                Latest decision on this market
              </RouterLink>
            </div>
            <Button
              size="sm"
              variant="outline"
              :loading="refreshing"
              @click="refresh"
            >
              <Icon name="lucide:refresh-cw" />
              Refresh brief
            </Button>
          </div>
          <BriefView
            :record="reader.data.value"
            :max-move="limits.maxPriceMove"
          />
        </div>
      </section>
    </div>
  </div>
</template>
