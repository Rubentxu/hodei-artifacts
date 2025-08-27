// Environment variable validation and utilities

import { z } from 'zod';

// Schema for environment variables
const envSchema = z.object({
  // API Configuration
  VITE_API_BASE_URL: z.string().url().default('http://localhost:8080'),
  VITE_API_TIMEOUT: z.coerce.number().positive().default(30000),

  // Application Configuration
  VITE_APP_NAME: z.string().default('Hodei Artifacts'),
  VITE_APP_VERSION: z.string().default('0.1.0'),
  VITE_BASE_PATH: z.string().default('/'),

  // Feature Flags
  VITE_ENABLE_ANALYTICS: z.coerce.boolean().default(false),
  VITE_ENABLE_DEBUG: z.coerce.boolean().default(false),
  VITE_ENABLE_PWA: z.coerce.boolean().default(false),

  // Development
  VITE_DEV_PROXY: z.coerce.boolean().default(true),

  // Build Configuration
  VITE_SOURCEMAP: z.coerce.boolean().default(false),

  // Mode
  MODE: z.enum(['development', 'production', 'test']).default('development'),

  // Node Environment
  NODE_ENV: z
    .enum(['development', 'production', 'test'])
    .default('development'),
});

// Type for validated environment variables
export type Env = z.infer<typeof envSchema>;

// Validate environment variables
const env = envSchema.parse({
  VITE_API_BASE_URL: import.meta.env.VITE_API_BASE_URL,
  VITE_API_TIMEOUT: import.meta.env.VITE_API_TIMEOUT,
  VITE_APP_NAME: import.meta.env.VITE_APP_NAME,
  VITE_APP_VERSION: import.meta.env.VITE_APP_VERSION,
  VITE_BASE_PATH: import.meta.env.VITE_BASE_PATH,
  VITE_ENABLE_ANALYTICS: import.meta.env.VITE_ENABLE_ANALYTICS,
  VITE_ENABLE_DEBUG: import.meta.env.VITE_ENABLE_DEBUG,
  VITE_ENABLE_PWA: import.meta.env.VITE_ENABLE_PWA,
  VITE_DEV_PROXY: import.meta.env.VITE_DEV_PROXY,
  VITE_SOURCEMAP: import.meta.env.VITE_SOURCEMAP,
  MODE: import.meta.env.MODE,
  NODE_ENV: import.meta.env.NODE_ENV,
});

// Environment utilities
export const isDevelopment = env.MODE === 'development';
export const isProduction = env.MODE === 'production';
export const isTest = env.MODE === 'test';

// Feature flags
export const featureFlags = {
  analytics: env.VITE_ENABLE_ANALYTICS,
  debug: env.VITE_ENABLE_DEBUG,
  pwa: env.VITE_ENABLE_PWA,
};

// Configuration export
export const config = {
  api: {
    baseUrl: env.VITE_API_BASE_URL,
    timeout: env.VITE_API_TIMEOUT,
  },
  app: {
    name: env.VITE_APP_NAME,
    version: env.VITE_APP_VERSION,
    basePath: env.VITE_BASE_PATH,
  },
  build: {
    sourcemap: env.VITE_SOURCEMAP,
  },
  dev: {
    proxy: env.VITE_DEV_PROXY,
  },
};

// Export validated environment
export default env;
