import { defineConfig, loadEnv } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { tanstackRouter } from '@tanstack/router-plugin/vite'

export default defineConfig(({ mode }) => {
  // LAPIDARY_API points the page at another api, so two stacks on one machine each get their own
  // page — which is how sharing is checked end to end. `loadEnv` reads it from the environment
  // without this config needing Node's types, and `vite preview` takes the same proxy.
  const { LAPIDARY_API } = loadEnv(mode, '.', 'LAPIDARY_')
  return {
    plugins: [
      tanstackRouter({
        target: 'react',
        autoCodeSplitting: true,
        // index.test.tsx sits beside the route it tests. Without this the plugin scans it
        // as a route file and warns "does not export a Route" on every single build, CI
        // included. The pattern excludes any test file next to a route, not just this one.
        routeFileIgnorePattern: '\\.test\\.tsx?$',
      }),
      react(),
      tailwindcss(),
    ],
    server: {
      proxy: {
        '/api': { target: LAPIDARY_API || 'http://localhost:8080', changeOrigin: true },
      },
    },
  }
})
