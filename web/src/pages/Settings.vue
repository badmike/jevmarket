<script setup lang="ts">
import { useQueryClient } from '@tanstack/vue-query'
import { toast } from 'vue-sonner'

import { api, ApiError, errorMessage } from '~/api/client'
import { keys, useConfig } from '~/api/queries'
import type { ConfigValue, ConfigView } from '~/api/types'
import ConfigField, { type FieldKind } from '~/components/config/ConfigField.vue'
import { type FieldGroup, type FieldMeta, groups as knownGroups } from '~/components/config/fields'
import QueryError from '~/components/QueryError.vue'
import { Alert, AlertDescription } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
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
/** Secrets to remove from the file. */
const cleared = ref(new Set<string>())
const fieldErrors = ref<Record<string, string>>({})
const saving = ref(false)

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
    fields: Object.fromEntries(all.filter((k) => !known.has(k)).map((k) => [k, { label: k }])),
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

watch(
  config,
  (view) => {
    if (!view) return
    drafts.value = Object.fromEntries(fields.value.map((f) => [f.name, f.server]))
    cleared.value = new Set()
  },
  { immediate: true }
)

const isDirty = (f: Field) => cleared.value.has(f.name) || (drafts.value[f.name] ?? f.server) !== f.server
const dirty = computed(() => fields.value.filter(isDirty))

const turnsLive = computed(() => {
  const f = fields.value.find((x) => x.name === 'dry_run')
  return !!f && f.server === true && drafts.value.dry_run === false
})

const discard = () => {
  drafts.value = Object.fromEntries(fields.value.map((f) => [f.name, f.server]))
  cleared.value = new Set()
  fieldErrors.value = {}
}

const save = async () => {
  const changes: Record<string, ConfigValue> = {}
  const errors: Record<string, string> = {}
  for (const f of dirty.value) {
    if (cleared.value.has(f.name)) {
      changes[f.name] = null
      continue
    }
    const draft = drafts.value[f.name] ?? f.server
    const value = fromDraft(draft, f)
    if (f.kind === 'number' && (typeof draft !== 'string' || draft.trim() === '' || Number.isNaN(value))) {
      errors[f.name] = 'Enter a number'
    } else if (f.meta.integer && !Number.isInteger(value)) {
      errors[f.name] = 'Enter a whole number'
    }
    // Back at the default: drop the key so it keeps following the built-in default.
    changes[f.name] = draft === f.defaultDraft ? null : value
  }
  fieldErrors.value = errors
  if (Object.keys(errors).length) return

  saving.value = true
  try {
    const view = await guarded(
      'Switch dry run off?',
      'Saving this makes every pass place real orders on Polymarket, inside the caps in this configuration.',
      (confirm) => api.patchConfig(changes, confirm)
    )
    if (view) {
      client.setQueryData(keys.config, view)
      toast.success(`Saved ${Object.keys(changes).length} setting${Object.keys(changes).length === 1 ? '' : 's'}`)
    }
  } catch (e) {
    if (e instanceof ApiError && e.body.fields) fieldErrors.value = e.body.fields
    else toast.error(errorMessage(e))
  } finally {
    saving.value = false
  }
}

onBeforeRouteLeave(() => !dirty.value.length || window.confirm('Discard unsaved changes?'))
</script>

<template>
  <div class="mx-auto grid max-w-4xl gap-6 pb-20">
    <header class="grid gap-1">
      <h1 class="text-2xl font-semibold text-primary">Settings</h1>
      <p class="text-sm break-all text-muted">
        Saved to {{ config?.path ?? 'the config file' }} and checked like <code>jevmarket config set</code>. Passes
        read it when they start.
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
    <form
      v-else
      class="grid gap-6"
      @submit.prevent="save"
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
            :error="fieldErrors[f.name]"
            :from-env="config?.env_overrides.includes(f.name) ?? false"
            :is-set="config?.secrets.find((s) => s.key === f.name)?.set && !cleared.has(f.name)"
            @update:model-value="drafts[f.name] = $event"
            @reset="drafts[f.name] = f.defaultDraft ?? f.server"
            @clear="cleared = new Set(cleared).add(f.name)"
          />
        </div>
      </Card>

      <div
        v-if="dirty.length"
        class="fixed inset-x-0 bottom-[calc(var(--shell-tabbar-height)+env(safe-area-inset-bottom))] z-30 border-t border-border bg-background/95 backdrop-blur-md lg:bottom-0 lg:left-56"
      >
        <div class="mx-auto flex max-w-4xl flex-wrap items-center gap-3 px-shell-gutter-x py-3">
          <p
            class="flex-1 text-sm"
            aria-live="polite"
          >
            {{ dirty.length }} unsaved {{ dirty.length === 1 ? 'change' : 'changes' }}
            <span
              v-if="turnsLive"
              class="block font-semibold text-destructive"
            >
              Dry run goes off: passes will place real orders.
            </span>
          </p>
          <Button
            type="button"
            variant="ghost"
            @click="discard"
          >
            Discard
          </Button>
          <Button
            type="submit"
            :variant="turnsLive ? 'destructive' : 'accent'"
            :loading="saving"
          >
            Save
          </Button>
        </div>
      </div>
    </form>
  </div>
</template>
