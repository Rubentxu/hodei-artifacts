// Common types used across the application

export interface User {
  id: string;
  email: string;
  name: string;
  role: 'admin' | 'user' | 'viewer';
  avatar?: string;
  createdAt: string;
  lastLogin?: string;
}

export interface Repository {
  id: string;
  name: string;
  description?: string;
  type: 'maven' | 'npm' | 'pypi' | 'docker';
  visibility: 'public' | 'private';
  packageCount: number;
  size: number;
  lastUpdated: string;
  url: string;
}

export interface Artifact {
  id: string;
  name: string;
  version: string;
  repositoryId: string;
  type: string;
  size: number;
  checksum: string;
  uploadedAt: string;
  uploadedBy: string;
  metadata?: Record<string, any>;
}

export interface ApiResponse<T> {
  data: T;
  message?: string;
  success: boolean;
}

export interface PaginatedResponse<T> {
  data: T[];
  total: number;
  page: number;
  limit: number;
  hasNext: boolean;
  hasPrev: boolean;
}

export type ApiError = {
  message: string;
  code: string;
  details?: Record<string, any>;
};

// Utility types
export type Optional<T, K extends keyof T> = Omit<T, K> & Partial<Pick<T, K>>;
export type Require<T, K extends keyof T> = T & Required<Pick<T, K>>;
