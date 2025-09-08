// Tipos generados basados en la especificación OpenAPI de docs/openapi/

// Repository Types
export interface Repository {
  id: string;
  name: string;
  description?: string;
  createdAt: string;
}

export interface RepositoryListResponse {
  total: number;
  items: Repository[];
}

export interface CreateRepositoryRequest {
  name: string;
  description?: string;
}

export interface UpdateRepositoryRequest {
  name?: string;
  description?: string;
}

// Artifact Types
export interface ArtifactUploadResponse {
  id: string;
  status: 'accepted' | 'duplicate';
  repositoryId: string;
}

export interface PresignedUrlResponse {
  url: string;
  expiresAt: string;
}

// User Types
export interface CreateUserCommand {
  username: string;
  email: string;
  password: string;
  attributes?: Record<string, any>;
}

export interface CreateUserResponse {
  id: string;
}

export interface UpdateUserAttributesCommand {
  user_id: string;
  attributes: Record<string, any>;
}

export interface UpdateUserAttributesResponse {
  user_id: string;
}

// Search Types
export interface SearchResults {
  total: number;
  items: PackageResult[];
}

export interface PackageResult {
  id: string;
  name: string;
  version: string;
  description?: string;
  repositoryId: string;
  repositoryName: string;
  packageType: 'maven' | 'npm' | 'pypi';
  createdAt: string;
  downloadCount: number;
}

// Token Types
export interface TokenRequest {
  name: string;
  expiresIn?: number;
  permissions?: string[];
}

export interface TokenResponse {
  token: string;
  id: string;
  name: string;
  createdAt: string;
  expiresAt?: string;
}

export interface TokenInfo {
  id: string;
  name: string;
  createdAt: string;
  expiresAt?: string;
  lastUsedAt?: string;
  permissions: string[];
}

// Error Types
export interface ApiError {
  error: {
    code: string;
    message: string;
    details?: any;
  };
}

// Package-specific Types
export interface NpmPackageMetadata {
  name: string;
  'dist-tags': Record<string, string>;
  versions: Record<string, any>;
  time: Record<string, string>;
  maintainers: NpmMaintainer[];
  description?: string;
}

export interface NpmMaintainer {
  name: string;
  email?: string;
}

export interface NpmPublishRequest {
  name: string;
  version: string;
  description?: string;
  main?: string;
  scripts?: Record<string, string>;
  dependencies?: Record<string, string>;
  devDependencies?: Record<string, string>;
}

export interface NpmPublishResponse {
  success: boolean;
  id: string;
}

export interface PythonPackageUpload {
  name: string;
  version: string;
  summary?: string;
  description?: string;
  author?: string;
  author_email?: string;
  keywords?: string[];
}

// Policy Types
export interface CreatePolicyCommand {
  name: string;
  description?: string;
  policy: any; // JSON policy document
}

export interface CreatePolicyResponse {
  id: string;
  name: string;
  createdAt: string;
}

// API Response Types
export interface ApiResponse<T> {
  data: T;
  message?: string;
  timestamp: string;
}

export interface PaginatedResponse<T> {
  data: T[];
  pagination: {
    page: number;
    limit: number;
    total: number;
    totalPages: number;
    hasNext: boolean;
    hasPrevious: boolean;
  };
}

// Common Query Parameters
export interface SearchQuery {
  q?: string;
  type?: 'maven' | 'npm' | 'pypi';
  repositoryId?: string;
  page?: number;
  limit?: number;
  sortBy?: 'name' | 'createdAt' | 'downloadCount';
  sortOrder?: 'asc' | 'desc';
}

export interface FilterOptions {
  repositories?: string[];
  types?: ('maven' | 'npm' | 'pypi')[];
  dateRange?: {
    start?: string;
    end?: string;
  };
  sizeRange?: {
    min?: number;
    max?: number;
  };
}
