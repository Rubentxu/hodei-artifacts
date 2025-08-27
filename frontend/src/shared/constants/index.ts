// Application constants

// API Configuration
export const API_BASE_URL =
  import.meta.env.VITE_API_BASE_URL || 'http://localhost:8080';
export const API_TIMEOUT = 30000;

// Pagination
export const DEFAULT_PAGE_SIZE = 20;
export const MAX_PAGE_SIZE = 100;

// Repository Types
export const REPOSITORY_TYPES = {
  MAVEN: 'maven',
  NPM: 'npm',
  PYPI: 'pypi',
  DOCKER: 'docker',
} as const;

export type RepositoryType =
  (typeof REPOSITORY_TYPES)[keyof typeof REPOSITORY_TYPES];

// User Roles
export const USER_ROLES = {
  ADMIN: 'admin',
  USER: 'user',
  VIEWER: 'viewer',
} as const;

export type UserRole = (typeof USER_ROLES)[keyof typeof USER_ROLES];

// Visibility Options
export const VISIBILITY_OPTIONS = {
  PUBLIC: 'public',
  PRIVATE: 'private',
} as const;

export type Visibility =
  (typeof VISIBILITY_OPTIONS)[keyof typeof VISIBILITY_OPTIONS];

// File Upload
export const MAX_FILE_SIZE = 100 * 1024 * 1024; // 100MB
export const ALLOWED_FILE_TYPES = [
  '.jar',
  '.war',
  '.ear',
  '.pom',
  '.tgz',
  '.tar.gz',
  '.whl',
  '.egg',
  '.zip',
];

// Search
export const SEARCH_DEBOUNCE_MS = 300;
export const MIN_SEARCH_LENGTH = 2;
export const MAX_SEARCH_RESULTS = 50;

// Theme
export const THEMES = {
  LIGHT: 'light',
  DARK: 'dark',
  SYSTEM: 'system',
} as const;

export type Theme = (typeof THEMES)[keyof typeof THEMES];

// Storage Keys
export const STORAGE_KEYS = {
  THEME: 'hodei-theme',
  AUTH_TOKEN: 'hodei-auth-token',
  USER_PREFERENCES: 'hodei-user-preferences',
  RECENT_SEARCHES: 'hodei-recent-searches',
} as const;

// HTTP Status Codes
export const HTTP_STATUS = {
  OK: 200,
  CREATED: 201,
  NO_CONTENT: 204,
  BAD_REQUEST: 400,
  UNAUTHORIZED: 401,
  FORBIDDEN: 403,
  NOT_FOUND: 404,
  CONFLICT: 409,
  UNPROCESSABLE_ENTITY: 422,
  INTERNAL_SERVER_ERROR: 500,
} as const;

// Query Keys (for React Query)
export const QUERY_KEYS = {
  USERS: ['users'] as const,
  USER: (id: string) => ['users', id] as const,
  REPOSITORIES: ['repositories'] as const,
  REPOSITORY: (id: string) => ['repositories', id] as const,
  ARTIFACTS: ['artifacts'] as const,
  ARTIFACT: (id: string) => ['artifacts', id] as const,
  SEARCH: (query: string, filters?: Record<string, any>) =>
    ['search', query, filters] as const,
} as const;

// Error Messages
export const ERROR_MESSAGES = {
  NETWORK_ERROR: 'Network error. Please check your connection.',
  UNAUTHORIZED: 'You are not authorized to perform this action.',
  FORBIDDEN: 'Access denied.',
  NOT_FOUND: 'The requested resource was not found.',
  SERVER_ERROR: 'Internal server error. Please try again later.',
  VALIDATION_ERROR: 'Please check your input and try again.',
  FILE_TOO_LARGE: `File size must be less than ${MAX_FILE_SIZE / (1024 * 1024)}MB`,
  INVALID_FILE_TYPE: 'Invalid file type. Please upload a valid artifact.',
} as const;

// Success Messages
export const SUCCESS_MESSAGES = {
  CREATED: 'Successfully created!',
  UPDATED: 'Successfully updated!',
  DELETED: 'Successfully deleted!',
  UPLOADED: 'File uploaded successfully!',
  COPIED: 'Copied to clipboard!',
} as const;

// Validation Rules
export const VALIDATION_RULES = {
  EMAIL: /^[^\s@]+@[^\s@]+\.[^\s@]+$/,
  PASSWORD_MIN_LENGTH: 8,
  USERNAME_MIN_LENGTH: 3,
  USERNAME_MAX_LENGTH: 50,
  REPOSITORY_NAME: /^[a-zA-Z0-9]([a-zA-Z0-9\-._]*[a-zA-Z0-9])?$/,
  ARTIFACT_VERSION: /^[0-9]+\.[0-9]+\.[0-9]+([+-][a-zA-Z0-9\-._]+)?$/,
} as const;

// Date Formats
export const DATE_FORMATS = {
  SHORT: 'MMM dd, yyyy',
  LONG: 'MMMM dd, yyyy',
  WITH_TIME: 'MMM dd, yyyy HH:mm',
  ISO: 'yyyy-MM-dd',
} as const;

// Notification Types
export const NOTIFICATION_TYPES = {
  SUCCESS: 'success',
  ERROR: 'error',
  WARNING: 'warning',
  INFO: 'info',
} as const;

export type NotificationType =
  (typeof NOTIFICATION_TYPES)[keyof typeof NOTIFICATION_TYPES];

// Animation Durations (in milliseconds)
export const ANIMATION_DURATION = {
  FAST: 150,
  NORMAL: 300,
  SLOW: 500,
} as const;

// Breakpoints (matching Tailwind CSS)
export const BREAKPOINTS = {
  SM: 640,
  MD: 768,
  LG: 1024,
  XL: 1280,
  '2XL': 1536,
} as const;
