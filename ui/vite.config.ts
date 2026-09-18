import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { defineConfig } from 'vitest/config'

export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  test: { environment: 'happy-dom' },
  server: {
    port: 1420,
    strictPort: true,
  },
})
