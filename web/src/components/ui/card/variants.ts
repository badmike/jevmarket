import type { VariantProps } from 'class-variance-authority'
import { cva } from 'class-variance-authority'

export const cardVariants = cva('', {
  variants: {
    variant: {
      none: 'pb-12',
      default: 'rounded-xl p-6 max-sm:px-4 bg-card text-card-foreground shadow-soft',
      surface: 'rounded-xl p-6 max-sm:px-4 bg-surface shadow-soft',
      outline: 'rounded-xl p-6 max-sm:px-4 border border-border',
      accent: 'rounded-xl p-6 max-sm:px-4 bg-accent text-accent-foreground shadow-soft',
      warning: 'rounded-xl p-6 max-sm:px-4 bg-warning-background/20 text-warning shadow-sm',
      destructive:
        'rounded-xl p-6 max-sm:px-4 bg-destructive-background/20 text-destructive shadow-sm',
      destructiveOutline: 'rounded-xl p-6 max-sm:px-4 border border-destructive text-destructive',
    },
  },
  defaultVariants: {
    variant: 'default',
  },
})

export type CardVariants = VariantProps<typeof cardVariants>
