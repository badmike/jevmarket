import { createSharedComposable } from '@vueuse/core'

export const colorModes = ['light', 'dark', 'system'] as const

export type ColorMode = (typeof colorModes)[number]

/** Shared with the inline script in `index.html`, which applies the mode before first paint. */
const STORAGE_KEY = 'jevmarket:color-mode'

const isColorMode = (value: unknown): value is ColorMode => colorModes.includes(value as ColorMode)

const readStored = (): ColorMode => {
  try {
    const stored = localStorage.getItem(STORAGE_KEY)
    return isColorMode(stored) ? stored : 'system'
  } catch {
    // Private-mode Safari throws on access rather than returning null.
    return 'system'
  }
}

const useColorModeBase = () => {
  const mode = ref<ColorMode>(readStored())
  const media = window.matchMedia('(prefers-color-scheme: dark)')
  const prefersDark = ref(media.matches)
  media.addEventListener('change', (event) => {
    prefersDark.value = event.matches
  })

  const isDark = computed(() => (mode.value === 'system' ? prefersDark.value : mode.value === 'dark'))

  watchEffect(() => {
    document.documentElement.classList.toggle('dark', isDark.value)
    // Native widgets (scrollbars, form controls) read this, not the class.
    document.documentElement.style.colorScheme = isDark.value ? 'dark' : 'light'
  })

  const setMode = (next: ColorMode): void => {
    mode.value = next
    try {
      localStorage.setItem(STORAGE_KEY, next)
    } catch {
      /* the mode still applies for this session */
    }
  }

  return { mode, isDark, setMode }
}

export const useColorMode = createSharedComposable(useColorModeBase)
