import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// Tauri serves the built files itself; the dev server only needs a fixed
// port for `tauri dev` to point the webview at.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  // The aerodrome catalogue (~1.2 MB), fallback basemap (~1.5 MB) and MapLibre are
  // bundled into the JS; fine for a desktop app that loads from disk.
  build: {
    chunkSizeWarningLimit: 5000,
  },
  // MapLibre's worker uses ES module imports.
  worker: {
    format: 'es',
  },
  server: {
    port: 1420,
    strictPort: true,
  },
});
