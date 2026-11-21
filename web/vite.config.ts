import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// In dev, run the Rust server on :8080 and `npm run dev` here; requests are proxied.
const target = process.env.RUSTFLARE_URL ?? 'http://127.0.0.1:8080'
export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      '/api/ws': { target, ws: true },
      '/api': target,
      '/stream.mjpg': target,
      '/snapshot.jpg': target,
      '/files': target,
    },
  },
})
