import { resolve } from 'path';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { defineConfig } from 'vite';

const apiTarget = process.env.VITE_DIFIT_API_URL || 'http://localhost:4966';

export default defineConfig({
  plugins: [svelte()],
  root: 'src/client',
  publicDir: '../../public',
  build: {
    outDir: '../../dist/client',
    emptyOutDir: true,
  },
  resolve: {
    alias: {
      '@': resolve(import.meta.dirname, 'src'),
      '@/types': resolve(import.meta.dirname, 'src/types'),
    },
  },
  css: {
    postcss: './postcss.config.js',
  },
  server: {
    port: 5173,
    proxy: {
      '/api': {
        target: apiTarget,
        changeOrigin: true,
      },
    },
  },
});
