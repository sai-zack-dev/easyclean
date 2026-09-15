import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

/**
 * The release the UI belongs to, read from the workspace `Cargo.toml` so the
 * app and the server can only ever report the same number. Setting shows it
 * next to the version the server reports, which is how you tell from a phone
 * whether a deploy actually reached it.
 */
function appVersion(): string {
  try {
    const root = fileURLToPath(new URL('../Cargo.toml', import.meta.url))
    return readFileSync(root, 'utf8').match(/^version\s*=\s*"([^"]+)"/m)?.[1] ?? 'dev'
  } catch {
    return 'dev'
  }
}

export default defineConfig({
  define: {
    __APP_VERSION__: JSON.stringify(appVersion()),
  },
  plugins: [react(), tailwindcss()],
  server: {
    proxy: {
      '/api': {
        target: 'http://localhost:8080',
        changeOrigin: true,
      },
    },
  },
  build: {
    outDir: 'dist',
  },
})
