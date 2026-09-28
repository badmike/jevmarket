export type SortDirection = 'asc' | 'desc'

export interface TableSort {
  column: string
  direction: SortDirection
}

/** A column's sort value. Strings compare by locale, numbers by size. */
export type SortKey<T> = (row: T) => number | string

/** `rows` sorted by the key of `sort.column`, else of `fallback`. */
export function sortRows<T>(rows: T[], keys: Record<string, SortKey<T>>, sort: TableSort, fallback: string): T[] {
  const key = keys[sort.column] ?? keys[fallback]
  if (!key) return rows
  const dir = sort.direction === 'asc' ? 1 : -1
  return rows.toSorted((a, b) => {
    const x = key(a)
    const y = key(b)
    const order = typeof x === 'string' && typeof y === 'string' ? x.localeCompare(y) : Number(x) - Number(y)
    return (order || 0) * dir
  })
}
