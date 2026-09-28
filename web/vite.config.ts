import { readdirSync, readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { join, resolve } from 'node:path'

import tailwindcss from '@tailwindcss/vite'
import vue from '@vitejs/plugin-vue'
import AutoImport from 'unplugin-auto-import/vite'
import { defineConfig, type Plugin } from 'vite'

/** The daemon `bun run dev` proxies to. `JEVMARKET_URL` points elsewhere. */
const DAEMON = process.env.JEVMARKET_URL ?? 'http://127.0.0.1:8787'

const SRC = resolve(import.meta.dirname, 'src')

/**
 * `virtual:lucide-icons`: only the lucide icons the source names (`'lucide:…'` literals), about
 * 5 kB instead of the 600 kB set. The console has no icon picker, so every name is in the code.
 */
function lucideSubset(): Plugin {
  const id = 'virtual:lucide-icons'
  return {
    name: 'lucide-subset',
    resolveId: (source) => (source === id ? `\0${id}` : undefined),
    load(source) {
      if (source !== `\0${id}`) return
      const names = new Set<string>()
      for (const file of readdirSync(SRC, { recursive: true, encoding: 'utf8' })) {
        if (!/\.(vue|ts)$/.test(file)) continue
        const path = join(SRC, file)
        this.addWatchFile(path)
        for (const m of readFileSync(path, 'utf8').matchAll(/lucide:([a-z0-9-]+)/g)) names.add(m[1]!)
      }
      const require = createRequire(import.meta.url)
      const set = JSON.parse(readFileSync(require.resolve('@iconify-json/lucide/icons.json'), 'utf8'))
      const icons: Record<string, unknown> = {}
      const aliases: Record<string, unknown> = {}
      for (const name of names) {
        const alias = set.aliases?.[name]
        if (alias) {
          aliases[name] = alias
          icons[alias.parent] = set.icons[alias.parent]
        } else if (set.icons[name]) {
          icons[name] = set.icons[name]
        } else {
          this.warn(`unknown icon lucide:${name}`)
        }
      }
      const subset = { prefix: 'lucide', width: set.width, height: set.height, icons, aliases }
      return `export default ${JSON.stringify(subset)}`
    },
  }
}

export default defineConfig({
  // Relative asset URLs: the daemon rewrites `<base href>` in index.html to its `--base-path`,
  // so one build works at the root and under any proxy prefix.
  base: './',
  plugins: [
    // Only the framework APIs, as in werkstatt. Composables are imported explicitly.
    AutoImport({
      imports: ['vue', 'vue-router'],
      dts: 'src/types/auto-imports.d.ts',
      vueTemplate: true,
    }),
    vue(),
    tailwindcss(),
    lucideSubset(),
  ],
  resolve: {
    alias: {
      '~': SRC,
    },
  },
  server: {
    port: 5173,
    strictPort: true,
    // The API rejects writes whose Origin is not its own host, so keep the Host header as is.
    proxy: {
      '/api': { target: DAEMON, changeOrigin: false },
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    target: 'baseline-widely-available',
    rolldownOptions: {
      output: {
        codeSplitting: {
          groups: [
            { name: 'vue', test: /node_modules[\\/](vue|@vue|vue-router)[\\/]/ },
            { name: 'query', test: /node_modules[\\/]@tanstack[\\/]/ },
            { name: 'reka', test: /node_modules[\\/]reka-ui[\\/]/ },
          ],
        },
      },
    },
  },
})
