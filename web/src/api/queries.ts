import { useQuery } from '@tanstack/vue-query'
import type { MaybeRefOrGetter } from 'vue'

import { api } from '~/api/client'

/** Query keys in one place, so the live stream and the pages agree on them. */
export const keys = {
  status: ['status'],
  config: ['config'],
  recommendations: ['recommendations'],
  recommendation: (slug: string) => ['recommendations', slug],
  briefs: ['briefs'],
  brief: (id: number) => ['briefs', id],
  positions: ['positions'],
  orders: ['orders'],
  stats: ['stats'],
} as const

export const useStatus = () => useQuery({ queryKey: keys.status, queryFn: api.status })

export const useConfig = () => useQuery({ queryKey: keys.config, queryFn: api.config })

export const useRecommendations = () =>
  useQuery({ queryKey: keys.recommendations, queryFn: api.recommendations })

export const useRecommendation = (slug: MaybeRefOrGetter<string | null>) =>
  useQuery({
    queryKey: computed(() => keys.recommendation(toValue(slug) ?? '')),
    queryFn: () => api.recommendation(toValue(slug) ?? ''),
    enabled: () => !!toValue(slug),
  })

export const useBriefs = () => useQuery({ queryKey: keys.briefs, queryFn: api.briefs })

export const useBrief = (id: MaybeRefOrGetter<number | null>) =>
  useQuery({
    queryKey: computed(() => keys.brief(toValue(id) ?? 0)),
    queryFn: () => api.brief(toValue(id) ?? 0),
    enabled: () => toValue(id) != null,
  })

/** Reads the wallet from Polymarket: slower, so it stays fresh for a minute. */
export const usePositions = () =>
  useQuery({ queryKey: keys.positions, queryFn: api.positions, staleTime: 60_000 })

export const useOrders = () => useQuery({ queryKey: keys.orders, queryFn: api.orders })

export const useStats = () => useQuery({ queryKey: keys.stats, queryFn: api.stats })
