import { clsx, type ClassValue } from 'clsx'
import { twMerge } from 'tailwind-merge'

/** clsx plus tailwind-merge, so `cn('p-2', 'p-4')` renders one padding, not both. */
export const cn = (...inputs: ClassValue[]): string => twMerge(clsx(inputs))
