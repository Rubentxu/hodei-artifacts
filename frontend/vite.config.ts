import { defineConfig, loadEnv } from 'vite';
import react from '@vitejs/plugin-react';
import { resolve } from 'path';

// https://vite.dev/config/
export default defineConfig(({ mode }) => {
  // Load env file based on `mode` in the current working directory
  const env = loadEnv(mode, process.cwd(), '');
  
  return {
    plugins: [react()],
    
    // Base public path when served in production
    base: env.VITE_BASE_PATH || '/',
    
    // Build configuration
    build: {
      outDir: 'dist',
      sourcemap: mode !== 'production',
      minify: mode === 'production' ? 'esbuild' : false,
      rollupOptions: {
        output: {
          manualChunks: {
            vendor: ['react', 'react-dom'],
            ui: ['@tanstack/react-query', 'zustand'],
            utils: ['axios', 'clsx', 'tailwind-merge'],
          },
        },
      },
      chunkSizeWarningLimit: 1000,
    },
    
    // Preview server configuration
    preview: {
      port: 4173,
      host: true,
      strictPort: true,
    },
    
    // Development server configuration
    server: {
      port: 5173,
      host: true,
      strictPort: true,
      open: true,
      proxy: {
        '/api': {
          target: env.VITE_API_BASE_URL || 'http://localhost:8080',
          changeOrigin: true,
          secure: false,
        },
      },
    },
    
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
    
    // CSS configuration
    css: {
      devSourcemap: true,
    },
    
    // Optimize dependencies
    optimizeDeps: {
      include: ['react', 'react-dom'],
      exclude: ['@tanstack/react-query', 'zustand'],
    },
  };
});
