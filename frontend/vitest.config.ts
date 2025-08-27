import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';
import { resolve } from 'path';

export default defineConfig({
  plugins: [react()],

  test: {
    // Test environment
    environment: 'jsdom',

    // Global test setup
    setupFiles: ['./src/shared/test/setup.ts'],

    // Globals configuration for expect, vi, etc.
    globals: true,

    // Include patterns
    include: [
      'src/**/*.{test,spec}.{js,mjs,cjs,ts,mts,cts,jsx,tsx}',
      'src/**/__tests__/**/*.{js,mjs,cjs,ts,mts,cts,jsx,tsx}',
    ],

    // Exclude patterns
    exclude: [
      '**/node_modules/**',
      '**/dist/**',
      '**/build/**',
      '**/.{idea,git,cache,output,temp}/**',
      '**/{karma,rollup,webpack,vite,vitest,jest,ava,babel,nyc,cypress,tsup,build}.config.*',
    ],

    // Coverage configuration
    coverage: {
      provider: 'v8',
      reporter: ['text', 'json', 'html'],
      exclude: [
        'coverage/**',
        'dist/**',
        '**/node_modules/**',
        '**/[.]**',
        '**/*.d.ts',
        '**/virtual:*',
        '**/__x00__*',
        '**/\x00*',
        'cypress/**',
        'test?(s)/**',
        'test?(-*).?(c|m)[jt]s?(x)',
        '**/*{.,-}test.?(c|m)[jt]s?(x)',
        '**/*{.,-}spec.?(c|m)[jt]s?(x)',
        '**/__tests__/**',
        '**/{karma,rollup,webpack,vite,vitest,jest,ava,babel,nyc,cypress,tsup,build}.config.*',
        '**/vitest.config.*',
        '**/vite.config.*',
        // App specific exclusions
        'src/main.tsx',
        'src/vite-env.d.ts',
        'src/**/*.stories.{js,jsx,ts,tsx}',
        'src/shared/test/**',
      ],
      all: true,
      lines: 80,
      functions: 80,
      branches: 80,
      statements: 80,
    },

    // Test timeout
    testTimeout: 10000,
    hookTimeout: 10000,

    // Watch options
    watch: {
      exclude: ['**/node_modules/**', '**/dist/**'],
    },

    // Reporter configuration
    reporter: ['verbose'],

    // Mock configuration
    mockReset: true,
    clearMocks: true,
    restoreMocks: true,
  },

  // Resolve configuration matching main vite config
  resolve: {
    alias: {
      '@': resolve(__dirname, './src'),
      '@components': resolve(__dirname, './src/components'),
      '@features': resolve(__dirname, './src/features'),
      '@pages': resolve(__dirname, './src/pages'),
      '@shared': resolve(__dirname, './src/shared'),
      '@hooks': resolve(__dirname, './src/shared/hooks'),
      '@stores': resolve(__dirname, './src/shared/stores'),
      '@api': resolve(__dirname, './src/shared/api'),
      '@utils': resolve(__dirname, './src/shared/utils'),
      '@types': resolve(__dirname, './src/shared/types'),
    },
  },
});
