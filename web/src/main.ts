import { addCollection } from '@iconify/vue'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createApp } from 'vue'

import lucide from 'virtual:lucide-icons'

import App from '~/app.vue'
import { router } from '~/router'

import '~/assets/css/app.css'

// Icons render from the bundled subset: no request to the Iconify API, works offline.
addCollection(lucide)

const queryClient = new QueryClient({
  defaultOptions: {
    // The event stream invalidates what changed, so polling is not needed.
    queries: { staleTime: 30_000, retry: 1 },
  },
})

createApp(App).use(VueQueryPlugin, { queryClient }).use(router).mount('#app')
