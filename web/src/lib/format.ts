/** Formatters for the console. English only, cached `Intl` instances. */

const usd2 = new Intl.NumberFormat('en', { style: 'currency', currency: 'USD' })
const usd4 = new Intl.NumberFormat('en', {
  style: 'currency',
  currency: 'USD',
  minimumFractionDigits: 2,
  maximumFractionDigits: 4,
})
const int = new Intl.NumberFormat('en')
const dateTime = new Intl.DateTimeFormat('en', { dateStyle: 'medium', timeStyle: 'short' })
const clock = new Intl.DateTimeFormat('en', { timeStyle: 'medium' })
const calendarDay = new Intl.DateTimeFormat('en', { dateStyle: 'medium', timeZone: 'UTC' })
const rel = new Intl.RelativeTimeFormat('en', { numeric: 'auto' })

const DASH = '–'

/** `$4.20`; small amounts like OpenRouter spend keep up to four decimals. */
export const usd = (v: number | null | undefined, precise = false): string =>
  v == null ? DASH : (precise ? usd4 : usd2).format(v)

export const signedUsd = (v: number): string => `${v > 0 ? '+' : ''}${usd2.format(v)}`

export const count = (v: number | null | undefined): string => (v == null ? DASH : int.format(v))

/** A price or probability in [0, 1], e.g. `0.42`. */
export const prob = (v: number | null | undefined, digits = 2): string =>
  v == null ? DASH : v.toFixed(digits)

/** Percentage points, e.g. `+8.0 pts` for an edge of 0.08. */
export const points = (v: number | null | undefined): string =>
  v == null ? DASH : `${v > 0 ? '+' : ''}${(v * 100).toFixed(1)} pts`

export const percent = (v: number | null | undefined): string =>
  v == null ? DASH : `${Math.round(v * 100)}%`

export const at = (ts: number | null | undefined): string =>
  ts == null ? DASH : dateTime.format(new Date(ts * 1000))

export const clockTime = (ts: number): string => clock.format(new Date(ts * 1000))

const dayStart = (date: string) => new Date(`${date}T00:00:00Z`)

/** `Oct 3, 2026` for a `YYYY-MM-DD` date. */
export const day = (date: string | null | undefined): string =>
  date ? calendarDay.format(dayStart(date)) : DASH

/** `today`, `tomorrow`, `in 12 days` or `3 days ago` for a `YYYY-MM-DD` date, against `now` (unix seconds). */
export const inDays = (date: string | null | undefined, now = Date.now() / 1000): string => {
  if (!date) return DASH
  return rel.format(Math.round(dayStart(date).getTime() / 86_400_000) - Math.floor(now / 86_400), 'day')
}

const UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ['day', 86400],
  ['hour', 3600],
  ['minute', 60],
  ['second', 1],
]

/** `3 minutes ago`, `in 12 minutes`, against `now` (unix seconds). */
export const ago = (ts: number | null | undefined, now = Date.now() / 1000): string => {
  if (ts == null) return DASH
  const diff = ts - now
  for (const [unit, secs] of UNITS) {
    if (Math.abs(diff) >= secs || unit === 'second') return rel.format(Math.round(diff / secs), unit)
  }
  return DASH
}

/** `4m 05s` until `ts`. */
export const countdown = (ts: number, now = Date.now() / 1000): string => {
  const left = Math.max(0, Math.round(ts - now))
  const m = Math.floor(left / 60)
  const s = String(left % 60).padStart(2, '0')
  return m >= 60 ? `${Math.floor(m / 60)}h ${String(m % 60).padStart(2, '0')}m` : `${m}m ${s}s`
}

/** `2026-09-20: fact` into its date and text; undated lines keep a null date. */
export const datedLine = (line: string): { date: string | null; text: string } => {
  const m = /^(\d{4}-\d{2}-\d{2})\s*[:–-]\s*(.*)$/s.exec(line)
  return m ? { date: m[1] ?? null, text: m[2] ?? '' } : { date: null, text: line }
}

export const hostname = (url: string): string => {
  try {
    return new URL(url).hostname.replace(/^www\./, '')
  } catch {
    return url
  }
}
