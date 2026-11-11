/// <reference types="vitest" />
import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';
import { resolve } from 'path';

/**
 * The test lane of the frontend.
 *
 * The component tests were written against this toolchain, but the manifest
 * declared no runner and no script, so they had never been executed by
 * anyone: a test that has never run is a comment, not a test. The runner,
 * the environment and the aliases are declared here so that they run the
 * same way locally and in CI.
 *
 * The aliases mirror `vite.config.ts`. A test that imports `@shared/...`
 * resolves the same way the application does, otherwise the tests would
 * exercise a different module graph than the one that ships.
 */
export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@shared': resolve(__dirname, './src/shared'),
      '@': resolve(__dirname, './src'),
    },
  },
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./src/shared/test/setup.ts'],
    include: ['src/**/*.{test,spec}.{ts,tsx}'],
    css: false,
  },
});
