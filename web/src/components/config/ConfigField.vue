<script setup lang="ts">
import type { FieldMeta } from '~/components/config/fields'
import Icon from '~/components/Icon.vue'
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
  saving: boolean
  /** Saved a moment ago. */
  saved: boolean
  error?: string
  /** An environment variable overrides this key. */
  fromEnv: boolean
  /** Secrets only: whether a value is stored. */
  isSet?: boolean
}>()

const model = defineModel<string | boolean>({ required: true })
const emit = defineEmits<{ reset: []; clear: []; commit: [] }>()

/** Enter saves like leaving the field does. */
const blurOnEnter = (event: KeyboardEvent) => {
  if (event.target instanceof HTMLElement) event.target.blur()
}

const id = computed(() => `setting-${props.name}`)
const describedBy = computed(() =>
  [props.meta.help || props.defaultDraft !== null ? `${id.value}-help` : null, props.error ? `${id.value}-error` : null]
    .filter(Boolean)
    .join(' ') || undefined
)
const atDefault = computed(() => props.defaultDraft === null || model.value === props.defaultDraft)
/** Secrets reset by clearing the stored value, everything else back to its default. */
const resettable = computed(() => (props.kind === 'secret' ? !!props.isSet : !atDefault.value))
const reset = () => (props.kind === 'secret' ? emit('clear') : emit('reset'))
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
      <div class="group flex min-h-7 items-center gap-2">
        <label
          :for="id"
          class="text-sm font-medium text-primary"
        >
          {{ meta.label }}
        </label>
        <Button
          v-if="resettable"
          variant="ghost"
          size="toolbar"
          :aria-label="kind === 'secret' ? `Clear ${meta.label}` : `Reset ${meta.label} to its default`"
          :title="kind === 'secret' ? 'Clear' : 'Reset to default'"
          class="text-muted hover:text-primary"
          @click="reset"
        >
          <Icon name="lucide:undo-2" />
        </Button>
        <span
          v-if="saving || dirty"
          class="size-1.5 rounded-full bg-accent"
          :class="saving && 'animate-pulse'"
          :aria-label="saving ? 'saving' : 'unsaved'"
        />
        <Icon
          v-else-if="saved"
          name="lucide:check"
          size="14"
          class="text-success"
          aria-label="saved"
        />
        <code
          class="font-mono text-2xs text-muted opacity-0 transition-opacity group-focus-within:opacity-100 group-hover:opacity-100"
        >
          {{ name }}
        </code>
      </div>
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
          @update:model-value="
            (on: boolean) => {
              model = on
              emit('commit')
            }
          "
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
          @blur="emit('commit')"
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
          @blur="emit('commit')"
          @keydown.enter.prevent="blurOnEnter"
        />
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
