import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// The dev server proxies the API to a running `motion-extractor serve` (default port 8090).
const backend = process.env.MX_BACKEND ?? 'http://127.0.0.1:8090';

export default defineConfig({
  plugins: [svelte()],
  server: {
    proxy: {
      '/api': {
        target: backend,
        changeOrigin: true,
        ws: true,
        configure: (proxy) => {
          // The backend rejects foreign origins; the dev server origin is ours.
          proxy.on('proxyReq', (req) => req.removeHeader('origin'));
          proxy.on('proxyReqWs', (req) => req.removeHeader('origin'));
        },
      },
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    target: 'es2022',
  },
  test: {
    include: ['src/**/*.test.ts'],
  },
});
