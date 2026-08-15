import { sveltekit } from '@sveltejs/kit/vite';
import { svelteTesting } from '@testing-library/svelte/vite';
import { defineConfig } from 'vitest/config';

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [sveltekit(), svelteTesting()],
  clearScreen: false,
  // MapLibre constructs its worker from an ESM URL at runtime. Vite's dev
  // optimizer can strand that generated worker beside a stale prebundle,
  // leaving the native Areas screen blank even though production is valid.
  optimizeDeps: {
    exclude: ['maplibre-gl']
  },
  server: {
    port: 1420,
    strictPort: true,
    host: host || '127.0.0.1',
    hmr: host ? { protocol: 'ws', host, port: 1421 } : undefined,
    watch: { ignored: ['**/src-tauri/**'] }
  },
  envPrefix: ['VITE_', 'TAURI_'],
  test: {
    environment: 'jsdom',
    setupFiles: ['./src/test/setup.ts']
  }
});
