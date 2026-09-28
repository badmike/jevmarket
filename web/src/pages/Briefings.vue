<script setup lang="ts">
import { onKeyStroke, useNow } from '@vueuse/core'
import { toast } from 'vue-sonner'

import { api, errorMessage } from '~/api/client'
import { useBrief, useBriefs } from '~/api/queries'
import type { BriefSummary } from '~/api/types'
import BriefStatus from '~/components/BriefStatus.vue'
import BriefView from '~/components/BriefView.vue'
import Icon from '~/components/Icon.vue'
import QueryError from '~/components/QueryError.vue'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Input } from '~/components/ui/input'
import { Skeleton } from '~/components/ui/skeleton'
import { Tabs, TabsList, TabsTrigger } from '~/components/ui/tabs'
import { useReadState } from '~/composables/useReadState'
import { useThresholds } from '~/composables/useSettings'
import { typing } from '~/composables/useShortcuts'
import { ago } from '~/lib/format'

const route = useRoute()
const router = useRouter()
const list = useBriefs()
const limits = useThresholds()
const { isRead, markRead } = useReadState('briefs')
const now = useNow({ interval: 30_000 })
const search = ref('')
/** Settled briefs (market resolved or past its end date) are history, kept out of the way. */
const view = ref<'open' | 'settled'>('open')

const selectedId = computed(() => {
  const id = Number(route.params.id)
  return Number.isInteger(id) && id > 0 ? id : null
})
const reader = useBrief(selectedId)

const briefs = computed(() => {
  const q = search.value.trim().toLowerCase()
  return (list.data.value ?? []).filter(
    (b) =>
      b.settled === (view.value === 'settled') &&
      (!q || (b.question ?? '').toLowerCase().includes(q) || b.slug.includes(q) || b.summary.toLowerCase().includes(q))
  )
})

const settledCount = computed(() => (list.data.value ?? []).filter((b) => b.settled).length)
const read = (b: BriefSummary) => isRead(b.slug, b.ts)
const unread = computed(() => briefs.value.filter((b) => !read(b)))
const markAllRead = () => markRead(unread.value.map((b) => ({ key: b.slug, ts: b.ts })))

const open = (id: number) => router.replace({ name: 'briefings', params: { id } })

/** Opening a brief reads it, and a deep link to a settled one switches to that tab. */
watch(
  () => reader.data.value?.id,
  () => {
    const b = reader.data.value
    if (!b) return
    markRead([{ key: b.slug, ts: b.ts }])
    view.value = b.settled ? 'settled' : 'open'
  },
  { immediate: true }
)

/** A new brief for the open market replaces the one on screen. */
watch(
  () => list.data.value,
  (all) => {
    const current = reader.data.value
    const newer = current && all?.find((b) => b.slug === current.slug && b.id !== current.id && b.ts > current.ts)
    if (newer) void open(newer.id)
  }
)

const nav = useTemplateRef('nav')
watch(selectedId, async () => {
  await nextTick()
  nav.value?.querySelector('[aria-current="page"]')?.scrollIntoView({ block: 'nearest' })
})

/** `j` and `k` open the next and previous brief in the list, like a mail reader. */
const step = (by: 1 | -1) => (event: KeyboardEvent) => {
  if (event.metaKey || event.ctrlKey || event.altKey || typing(event.target)) return
  const items = briefs.value
  if (!items.length) return
  const index = items.findIndex((b) => b.id === selectedId.value)
  const next = items[index < 0 ? 0 : Math.min(Math.max(index + by, 0), items.length - 1)]
  if (next && next.id !== selectedId.value) {
    event.preventDefault()
    void open(next.id)
  }
}
onKeyStroke('j', step(1), { dedupe: false })
onKeyStroke('k', step(-1), { dedupe: false })

const refreshing = ref(false)
const refresh = async () => {
  const slug = reader.data.value?.slug
  if (!slug) return
  refreshing.value = true
  try {
    const b = await api.refreshBrief(slug)
    toast.success('Brief refreshed')
    await open(b.id)
  } catch (e) {
    toast.error(errorMessage(e))
  } finally {
    refreshing.value = false
  }
}
</script>

<template>
  <div class="mx-auto grid max-w-7xl gap-4">
    <header
      class="grid gap-1"
      :class="selectedId !== null && 'max-lg:hidden'"
    >
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
      class="grid items-start gap-4 lg:grid-cols-[22rem_1fr]"
    >
      <aside
        class="grid content-start gap-3 lg:sticky lg:top-[calc(var(--shell-header-height)+1rem)]"
        :class="selectedId !== null && 'max-lg:hidden'"
      >
        <Input
          v-model="search"
          type="search"
          placeholder="Search briefs  /"
          aria-label="Search briefs"
        />
        <div class="flex items-center justify-between gap-2">
          <Tabs v-model="view">
            <TabsList aria-label="Open or settled briefs">
              <TabsTrigger value="open">Open {{ (list.data.value?.length ?? 0) - settledCount }}</TabsTrigger>
              <TabsTrigger value="settled">Settled {{ settledCount }}</TabsTrigger>
            </TabsList>
          </Tabs>
          <Button
            v-if="unread.length"
            size="xs"
            variant="ghost"
            class="text-muted hover:text-primary"
            @click="markAllRead"
          >
            <Icon name="lucide:check-check" />
            Mark all read
          </Button>
        </div>
        <div
          v-if="list.isPending.value"
          class="grid gap-1"
        >
          <div
            v-for="i in 6"
            :key="i"
            class="grid gap-2 p-3"
          >
            <Skeleton class="h-4 w-11/12" />
            <Skeleton class="h-3 w-1/2" />
          </div>
        </div>
        <nav
          v-else
          ref="nav"
          aria-label="Briefs"
          class="-mx-1 grid gap-0.5 px-1 lg:max-h-[calc(100svh-15rem)] lg:overflow-y-auto"
        >
          <RouterLink
            v-for="b in briefs"
            :key="b.id"
            :to="{ name: 'briefings', params: { id: b.id } }"
            class="group grid grid-cols-[0.375rem_1fr] gap-x-2.5 gap-y-1 rounded-lg border border-transparent px-3 py-2.5 transition-colors hover:bg-secondary/60"
            active-class="border-border bg-card shadow-soft-sm hover:bg-card"
          >
            <span
              class="mt-1.75 size-1.5 rounded-full"
              :class="!read(b) && 'bg-accent'"
              :aria-label="read(b) ? undefined : 'Unread'"
              :role="read(b) ? undefined : 'img'"
            />
            <span
              class="line-clamp-2 text-sm font-medium text-primary transition-opacity"
              :class="read(b) && 'opacity-60 group-aria-[current=page]:opacity-100'"
            >
              {{ b.question ?? b.slug }}
            </span>
            <span
              class="col-start-2 flex items-center gap-1.5 text-xs text-muted transition-opacity"
              :class="read(b) && 'opacity-60 group-aria-[current=page]:opacity-100'"
            >
              <BriefStatus
                :fresh="b.fresh"
                :settled="b.settled"
              />
              <span>{{ ago(b.ts, now.getTime() / 1000) }}</span>
              <span aria-hidden="true">·</span>
              <span class="tabular-nums">{{ b.facts }} facts, {{ b.sources }} sources</span>
            </span>
          </RouterLink>
          <p
            v-if="!briefs.length"
            class="px-3 py-6 text-center text-sm text-muted"
          >
            {{ search.trim() ? 'No brief matches your search.' : `No ${view} briefs.` }}
          </p>
        </nav>
      </aside>

      <section
        class="min-w-0 rounded-xl bg-card p-4 shadow-soft sm:p-8"
        :class="selectedId === null && 'max-lg:hidden'"
        aria-live="polite"
      >
        <EmptyState
          v-if="selectedId === null"
          icon="lucide:book-open"
          title="Pick a brief"
          description="Select a market on the left to read its evidence. Press j and k to move through the list."
          class="border-none"
        />
        <div
          v-else-if="reader.isPending.value"
          class="grid max-w-3xl gap-4"
        >
          <Skeleton class="h-7 w-2/3" />
          <Skeleton class="h-3 w-1/3" />
          <Skeleton class="h-20" />
          <Skeleton class="h-48" />
        </div>
        <QueryError
          v-else-if="reader.error.value"
          :error="reader.error.value"
          @retry="reader.refetch()"
        />
        <div
          v-else-if="reader.data.value"
          class="grid max-w-3xl gap-6"
        >
          <RouterLink
            :to="{ name: 'briefings' }"
            class="inline-flex items-center gap-1 justify-self-start text-sm text-muted hover:text-primary lg:hidden"
          >
            <Icon
              name="lucide:arrow-left"
              aria-hidden="true"
            />
            All briefs
          </RouterLink>
          <header class="grid gap-3">
            <h2 class="text-2xl font-semibold text-balance text-primary">
              {{ reader.data.value.question ?? reader.data.value.slug }}
            </h2>
            <div class="flex flex-wrap items-center gap-2">
              <Button
                size="sm"
                variant="outline"
                as-child
              >
                <RouterLink :to="{ name: 'markets', params: { slug: reader.data.value.slug } }">
                  <Icon name="lucide:list-checks" />
                  Latest decision
                </RouterLink>
              </Button>
              <Button
                size="sm"
                variant="ghost"
                :loading="refreshing"
                @click="refresh"
              >
                <Icon name="lucide:refresh-cw" />
                Refresh brief
              </Button>
            </div>
          </header>
          <BriefView
            :key="reader.data.value.id"
            :record="reader.data.value"
            :max-move="limits.maxPriceMove"
          />
        </div>
      </section>
    </div>
  </div>
</template>
