import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

// https://vite.dev/config/
export default defineConfig({
  // Served by the Rust server under /static/ (see crates/hakimi-server/src/api.rs).
  base: '/static/',
  plugins: [
    react(),
    tailwindcss(),
  ],
  build: {
    // Emit the production bundle into the server-embedded directory with stable
    // (unhashed) filenames so api.rs can `include_str!` them. CI has no node step,
    // so the build output is committed.
    outDir: '../crates/hakimi-webui/static',
    emptyOutDir: true,
    rollupOptions: {
      output: {
        entryFileNames: 'app.js',
        chunkFileNames: 'app-[name].js',
        assetFileNames: 'app.[ext]',
      },
    },
  },
  server: {
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:3005',
        changeOrigin: true,
      },
      '/v1': {
        target: 'http://127.0.0.1:3005',
        changeOrigin: true,
        ws: true,
      },
    },
  },
})
