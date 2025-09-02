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

export type RepositoryType = 'maven' | 'npm' | 'pypi' | 'docker';

export interface Repository {
  id: string;
  name: string;
  description?: string;
  type: RepositoryType;
  visibility: 'public' | 'private';
  isPublic: boolean;
  packageCount: number;
  size: number;
  lastUpdated: string;
  url: string;
}

export interface RepositoryFilters {
  type?: RepositoryType[];
  visibility?: string[];
  search?: string;
  status?: string; // Added
  page?: number;
  limit?: number;
  sortBy?: 'name' | 'packageCount' | 'size' | 'lastUpdated';
  sortOrder?: 'asc' | 'desc';
}

export interface CreateRepositoryRequest {
  name: string;
  description?: string;
  type: RepositoryType;
  visibility: 'public' | 'private';
  isPublic?: boolean; // Added
  settings?: Record<string, any>;
}

export interface UpdateRepositoryRequest {
  name?: string;
  description?: string;
  visibility?: 'public' | 'private';
  settings?: Record<string, any>;
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

export interface RepositoryMetrics {
  totalPackages: number;
  activeRepositories: number;
  onlineUsers: number;
  storageUsed: { value: number; unit: string };
}

export interface ActivityEvent {
  id: string;
  type: 'upload' | 'download' | 'create' | 'update' | 'delete';
  userId: string;
  userName: string;
  targetType: 'repository' | 'artifact' | 'user';
  targetId: string;
  targetName: string;
  timestamp: string;
  details?: Record<string, any>;
}

export interface DashboardData {
  metrics: RepositoryMetrics;
  recentRepositories: Repository[];
  recentActivity: ActivityEvent[];
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

export interface ApiError {
  message: string;
  code: string;
  details?: Record<string, any>;
}

// Utility types
export type Optional<T, K extends keyof T> = Omit<T, K> & Partial<Pick<T, K>>;
export type Require<T, K extends keyof T> = T & Required<Pick<T, K>>;
