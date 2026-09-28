<script setup lang="ts">

defineProps<{
    /** The mark alone, with the name kept for screen readers. */
    compact?: boolean
}>()
</script>

<template>
    <RouterLink :to="{ name: 'overview' }"
        class="flex h-9 items-center gap-2.5 rounded-lg px-2 font-semibold text-sidebar-accent-foreground transition-colors duration-200 ease-butter hover:bg-sidebar-accent"
        :class="compact && 'w-9 justify-center px-0'">
        <svg xmlns="http://www.w3.org/2000/svg" class="size-5" viewBox="0 0 284.62 243.05">
            <path fill="currentColor"
                d="M99.33,158.07l.06-96.94,70.28,70.21,75.36-75.29-.02,55.78-75.4,75.24-30.82-30.75c-.19,16.05-3.04,31.23-9.43,46.44l40.29,40.29,114.97-114.93-.03-128.05-39.43-.07-75.5,75.48L99.29,5.15l-79.01.04.03,39.53,39.48-.04v113.79c-1.78,22.2-19.04,40.73-41.31,40.95l-18.42.18-.07,39.56c50.65,5.5,97.49-27.97,99.33-81.07Z" />
        </svg>
        <span class="truncate" :class="compact && 'sr-only'">
            jevmarket
        </span>
    </RouterLink>
</template>
