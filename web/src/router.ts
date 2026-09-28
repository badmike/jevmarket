import { createRouter, createWebHistory } from 'vue-router'

declare module 'vue-router' {
  interface RouteMeta {
    title?: string
  }
}

/** The nav, in order. `key` is the second key of its `g` shortcut. */
export const navigation = [
  { name: 'overview', label: 'Overview', icon: 'lucide:layout-dashboard', key: 'o' },
  { name: 'markets', label: 'Markets', icon: 'lucide:list-checks', key: 'm' },
  { name: 'briefings', label: 'Briefings', icon: 'lucide:newspaper', key: 'b' },
  { name: 'positions', label: 'Positions', icon: 'lucide:wallet', key: 'p' },
  { name: 'stats', label: 'Stats', icon: 'lucide:chart-line', key: 's' },
  { name: 'settings', label: 'Settings', icon: 'lucide:sliders-horizontal', key: ',' },
] as const

export type PageName = (typeof navigation)[number]['name']

export const router = createRouter({
  // The directory `<base href>` points at, so routes live under the daemon's --base-path.
  history: createWebHistory(new URL(document.baseURI).pathname),
  routes: [
    { path: '/', name: 'overview', component: () => import('~/pages/Overview.vue'), meta: { title: 'Overview' } },
    {
      path: '/markets/:slug?',
      name: 'markets',
      component: () => import('~/pages/Markets.vue'),
      meta: { title: 'Markets' },
    },
    {
      path: '/briefings/:id?',
      name: 'briefings',
      component: () => import('~/pages/Briefings.vue'),
      meta: { title: 'Briefings' },
    },
    { path: '/positions', name: 'positions', component: () => import('~/pages/Positions.vue'), meta: { title: 'Positions' } },
    { path: '/stats', name: 'stats', component: () => import('~/pages/Stats.vue'), meta: { title: 'Stats' } },
    {
      path: '/settings',
      name: 'settings',
      component: () => import('~/pages/Settings.vue'),
      meta: { title: 'Settings' },
    },
    { path: '/:rest(.*)*', name: 'not-found', component: () => import('~/pages/NotFound.vue'), meta: { title: 'Not found' } },
  ],
})

router.afterEach((to) => {
  document.title = to.meta.title ? `${to.meta.title} · jevmarket` : 'jevmarket'
})
