<script setup lang="ts">
import { useQueryClient } from '@tanstack/vue-query'

import { api, ApiError, errorMessage } from '~/api/client'
import { keys, useConfig } from '~/api/queries'
import type { ConfigValue, ConfigView } from '~/api/types'
import ConfigField, { type FieldKind } from '~/components/config/ConfigField.vue'
import { type FieldGroup, type FieldMeta, groups as knownGroups } from '~/components/config/fields'
import QueryError from '~/components/QueryError.vue'
import { Alert, AlertDescription } from '~/components/ui/alert'
import { Card } from '~/components/ui/card'
import { Skeleton } from '~/components/ui/skeleton'
import { useLiveConfirm } from '~/composables/useLiveConfirm'

type Draft = string | boolean

interface Field {
  name: string
  meta: FieldMeta
  kind: FieldKind
  server: Draft
  defaultDraft: Draft | null
}

const { data: config, error, isPending, refetch } = useConfig()
const client = useQueryClient()
const { guarded } = useLiveConfirm()

const drafts = ref<Record<string, Draft>>({})
const fieldErrors = ref<Record<string, string>>({})
/** Fields with a save in flight, and fields saved a moment ago. */
const saving = ref(new Set<string>())
const saved = ref(new Set<string>())

const kindOf = (view: ConfigView, key: string): FieldKind => {
  if (view.secrets.some((s) => s.key === key)) return 'secret'
  const sample = view.defaults[key] ?? view.values[key]
  if (typeof sample === 'boolean') return 'bool'
  if (typeof sample === 'number') return 'number'
  return Array.isArray(sample) ? 'list' : 'text'
}

const toDraft = (value: ConfigValue | undefined, kind: FieldKind): Draft => {
  if (kind === 'bool') return value === true
  if (kind === 'secret' || value == null) return ''
  return Array.isArray(value) ? value.join('\n') : String(value)
}

const fromDraft = (draft: Draft, f: Field): ConfigValue => {
  if (typeof draft === 'boolean') return draft
  const text = draft.trim()
  if (f.kind === 'number') return Number(text)
  if (f.kind === 'list') return text.split('\n').map((l) => l.trim()).filter(Boolean)
  return text === '' && f.meta.nullable ? null : text
}

/** Known groups in README order, then anything newer under "Other". */
const sections = computed(() => {
  const view = config.value
  if (!view) return []
  const all = [...view.secrets.map((s) => s.key), ...Object.keys(view.values)]
  const known = new Set(knownGroups.flatMap((g) => Object.keys(g.fields)))
  const other: FieldGroup = {
    title: 'Other',
    fields: Object.fromEntries(all.filter((k) => !known.has(k)).map((k) => [k, { label: k, help: '' }])),
  }
  return [...knownGroups, other]
    .map((g) => ({
      ...g,
      fields: Object.entries(g.fields)
        .filter(([name]) => all.includes(name))
        .map(([name, meta]): Field => {
          const kind = kindOf(view, name)
          return {
            name,
            meta,
            kind,
            server: toDraft(view.values[name], kind),
            defaultDraft: kind === 'secret' ? null : toDraft(view.defaults[name], kind),
          }
        }),
    }))
    .filter((g) => g.fields.length)
})

const fields = computed(() => sections.value.flatMap((s) => s.fields))

/** Server values the drafts were last synced to. */
let synced: Record<string, Draft> = {}

/** Take the server's values, except where a draft is still being edited. */
watch(
  config,
  (view) => {
    if (!view) return
    const next: Record<string, Draft> = {}
    for (const f of fields.value) {
      const draft = drafts.value[f.name]
      next[f.name] = draft === undefined || draft === synced[f.name] ? f.server : draft
    }
    synced = Object.fromEntries(fields.value.map((f) => [f.name, f.server]))
    drafts.value = next
  },
  { immediate: true }
)

const isDirty = (f: Field) => (drafts.value[f.name] ?? f.server) !== f.server

const flag = (set: typeof saving, name: string, on: boolean) => {
  const next = new Set(set.value)
  if (on) next.add(name)
  else next.delete(name)
  set.value = next
}

/** Save one field: on blur, on a switch flip, or from Default and Clear. `null` removes the key. */
const commit = async (f: Field, remove = false) => {
  if (!remove && !isDirty(f)) return
  const draft = drafts.value[f.name] ?? f.server
  if (!remove && f.kind === 'secret' && draft === '') return
  const value = remove ? null : fromDraft(draft, f)
  const errors = { ...fieldErrors.value }
  delete errors[f.name]
  if (!remove && f.kind === 'number' && (typeof draft !== 'string' || draft.trim() === '' || Number.isNaN(value))) {
    errors[f.name] = 'Enter a number'
  } else if (!remove && f.meta.integer && !Number.isInteger(value)) {
    errors[f.name] = 'Enter a whole number'
  }
  fieldErrors.value = errors
  if (errors[f.name]) return

  // Back at the default: drop the key so it keeps following the built-in default.
  const changes = { [f.name]: remove || draft === f.defaultDraft ? null : value }
  flag(saving, f.name, true)
  try {
    const view = await guarded(
      'Switch dry run off?',
      'This makes every pass place real orders on Polymarket, inside the caps in this configuration.',
      (confirm) => api.patchConfig(changes, confirm)
    )
    if (view) {
      if (f.kind === 'secret') drafts.value[f.name] = ''
      client.setQueryData(keys.config, view)
      flag(saved, f.name, true)
      setTimeout(() => flag(saved, f.name, false), 2000)
    } else {
      drafts.value[f.name] = f.server
    }
  } catch (e) {
    if (e instanceof ApiError && e.body.fields) fieldErrors.value = { ...fieldErrors.value, ...e.body.fields }
    else fieldErrors.value = { ...fieldErrors.value, [f.name]: errorMessage(e) }
  } finally {
    flag(saving, f.name, false)
  }
}

const reset = (f: Field) => {
  drafts.value[f.name] = f.defaultDraft ?? f.server
  void commit(f)
}
</script>


<template>
  <div class="mx-auto grid max-w-4xl gap-6">
    <header class="grid gap-1">
      <h1 class="text-2xl font-semibold text-primary">Settings</h1>
      <p class="text-sm break-all text-muted">
        Each setting saves when you leave the field, to {{ config?.path ?? 'the config file' }}, checked like
        <code>jevmarket config set</code>. Passes read it when they start.
      </p>
    </header>

    <Alert
      v-if="config?.dry_run_forced"
      color="info"
      icon="lucide:flask-conical"
    >
      <AlertDescription>
        The daemon runs with --dry-run: no order is placed, whatever Dry run says here. The setting still applies to
        <code>jevmarket run</code>.
      </AlertDescription>
    </Alert>

    <QueryError
      v-if="error"
      :error="error"
      @retry="refetch()"
    />
    <template v-else-if="isPending">
      <Skeleton
        v-for="i in 3"
        :key="i"
        class="h-64 rounded-xl"
      />
    </template>
    <div
      v-else
      class="grid gap-6"
    >
      <Card
        v-for="section in sections"
        :key="section.title"
        class="grid gap-4"
      >
        <div class="grid gap-1">
          <h2 class="text-base font-semibold text-primary">{{ section.title }}</h2>
          <p
            v-if="section.description"
            class="text-sm text-muted"
          >
            {{ section.description }}
          </p>
        </div>
        <div class="divide-y divide-border">
          <ConfigField
            v-for="f in section.fields"
            :key="f.name"
            :model-value="drafts[f.name] ?? f.server"
            :name="f.name"
            :meta="f.meta"
            :kind="f.kind"
            :default-draft="f.defaultDraft"
            :dirty="isDirty(f)"
            :saving="saving.has(f.name)"
            :saved="saved.has(f.name)"
            :error="fieldErrors[f.name]"
            :from-env="config?.env_overrides.includes(f.name) ?? false"
            :is-set="config?.secrets.find((s) => s.key === f.name)?.set"
            @update:model-value="drafts[f.name] = $event"
            @commit="commit(f)"
            @reset="reset(f)"
            @clear="commit(f, true)"
          />
        </div>
      </Card>

    </div>
  </div>
</template>
