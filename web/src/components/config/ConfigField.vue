<script setup lang="ts">
import type { FieldMeta } from '~/components/config/fields'
import { Button } from '~/components/ui/button'
import { Input } from '~/components/ui/input'
import { Switch } from '~/components/ui/switch'
import { Textarea } from '~/components/ui/textarea'

export type FieldKind = 'secret' | 'bool' | 'number' | 'list' | 'text'

const props = defineProps<{
  name: string
  meta: FieldMeta
  kind: FieldKind
  /** The draft as it would read with the built-in default. */
  defaultDraft: string | boolean | null
  dirty: boolean
  error?: string
  /** An environment variable overrides this key. */
  fromEnv: boolean
  /** Secrets only: whether a value is stored. */
  isSet?: boolean
}>()

const model = defineModel<string | boolean>({ required: true })
defineEmits<{ reset: []; clear: [] }>()

const id = computed(() => `setting-${props.name}`)
const describedBy = computed(() =>
  [props.meta.help || props.defaultDraft !== null ? `${id.value}-help` : null, props.error ? `${id.value}-error` : null]
    .filter(Boolean)
    .join(' ') || undefined
)
const atDefault = computed(() => props.defaultDraft === null || model.value === props.defaultDraft)
const defaultText = computed(() => {
  const d = props.defaultDraft
  if (d === null || props.kind === 'list') return null
  if (typeof d === 'boolean') return d ? 'on' : 'off'
  return d === '' ? 'empty' : d
})
</script>

<template>
  <div class="grid gap-2 py-4 first:pt-0 last:pb-0 md:grid-cols-[minmax(0,1fr)_minmax(0,22rem)] md:gap-6">
    <div class="grid content-start gap-1">
      <label
        :for="id"
        class="flex items-center gap-2 text-sm font-medium text-primary"
      >
        {{ meta.label }}
        <span
          v-if="dirty"
          class="size-1.5 rounded-full bg-accent"
          aria-label="unsaved"
        />
      </label>
      <p class="font-mono text-2xs text-muted">{{ name }}</p>
      <p
        :id="`${id}-help`"
        class="text-sm text-muted"
      >
        {{ meta.help }}
        <template v-if="defaultText !== null">
          <span v-if="meta.help"> · </span>Default {{ defaultText }}
        </template>
      </p>
      <p
        v-if="fromEnv"
        class="text-sm text-warning"
      >
        Set by an environment variable, which wins over the file.
      </p>
    </div>
    <div class="grid content-start gap-1.5">
      <div class="flex items-center gap-2">
        <Switch
          v-if="kind === 'bool'"
          :id="id"
          :model-value="model === true"
          :aria-describedby="describedBy"
          @update:model-value="model = $event"
        />
        <Textarea
          v-else-if="kind === 'list'"
          :id="id"
          :model-value="String(model)"
          :rows="5"
          :aria-describedby="describedBy"
          :aria-invalid="!!error"
          spellcheck="false"
          @update:model-value="model = String($event)"
        />
        <Input
          v-else
          :id="id"
          :model-value="String(model)"
          :type="kind === 'secret' ? 'password' : kind === 'number' ? 'number' : 'text'"
          :step="kind === 'number' ? (meta.integer ? 1 : 'any') : undefined"
          :inputmode="kind === 'number' ? (meta.integer ? 'numeric' : 'decimal') : undefined"
          :placeholder="kind === 'secret' ? (isSet ? 'Set. Type to replace' : 'Not set') : meta.nullable ? 'Empty' : undefined"
          :autocomplete="kind === 'secret' ? 'new-password' : 'off'"
          :aria-describedby="describedBy"
          :aria-invalid="!!error"
          spellcheck="false"
          class="tabular-nums"
          @update:model-value="model = String($event)"
        />
        <Button
          v-if="kind !== 'secret' && !atDefault"
          variant="ghost"
          size="xs"
          @click="$emit('reset')"
        >
          Default
        </Button>
        <Button
          v-if="kind === 'secret' && isSet"
          variant="ghost"
          size="xs"
          @click="$emit('clear')"
        >
          Clear
        </Button>
      </div>
      <p
        v-if="error"
        :id="`${id}-error`"
        class="text-sm text-destructive"
      >
        {{ error }}
      </p>
    </div>
  </div>
</template>
