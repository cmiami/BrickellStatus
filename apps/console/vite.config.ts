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
  // MapLibre starts its worker with `{ type: 'module' }`, so the worker chunk
  // Vite emits for it has to be an ES module too.
  worker: {
    format: 'es'
  },
  server: {
    port: 1420,
    strictPort: true,
    host: host || '127.0.0.1',
    hmr: host ? { protocol: 'ws', host, port: 1421 } : undefined,
    watch: { ignored: ['**/src-tauri/**'] },
    // The shipped feed catalog lives beside the Rust crate that embeds it, so
    // both sides read one file and cannot drift. It sits outside the console
    // root, which the dev server would otherwise refuse to serve.
    fs: { allow: ['..', '../../crates/runtime/catalog'] }
  },
  envPrefix: ['VITE_', 'TAURI_'],
  test: {
    environment: 'jsdom',
    setupFiles: ['./src/test/setup.ts']
  }
});
