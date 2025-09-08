/**
 * Tipos TypeScript generados a partir del contrato OpenAPI 3.0.3
 * Este archivo contiene todos los tipos de datos según el contrato API
 * Generado el: $(date)
 */

// ===== ESQUEMAS DE REPOSITORY =====
export interface Repository {
  id: string; // uuid
  name: string;
  description?: string;
  createdAt: string; // date-time
}

export interface RepositoryListResponse {
  total?: number;
  items?: Repository[];
}

export interface CreateRepositoryRequest {
  name: string;
  description?: string;
}

export interface UpdateRepositoryRequest {
  name?: string;
  description?: string;
}

// ===== ESQUEMAS DE ARTIFACT =====
export interface ArtifactUploadResponse {
  id?: string;
  status?: 'accepted' | 'duplicate';
  repositoryId?: string; // uuid
}

export interface PresignedUrlResponse {
  url?: string; // uri
  expiresAt?: string; // date-time
}

// ===== ESQUEMAS DE SEARCH =====
export interface SearchResults {
  total?: number;
  limit?: number;
  offset?: number;
  results?: PackageResult[];
}

export interface PackageResult {
  type?: 'maven' | 'npm' | 'pypi';
  name?: string;
  latestVersion?: string;
  description?: string;
  downloadUrl?: string; // uri
  lastModified?: string; // date-time
  downloads?: number;
  maintainers?: string[];
  keywords?: string[];
  license?: string;
  score?: number; // float
}

// ===== ESQUEMAS DE NPM =====
export interface NpmPackageMetadata {
  name?: string;
  'dist-tags'?: {
    [key: string]: string;
  };
  versions?: {
    [key: string]: NpmVersionMetadata;
  };
  time?: {
    [key: string]: string; // date-time
  };
  maintainers?: NpmMaintainer[];
  description?: string;
  keywords?: string[];
  license?: string;
  repository?: {
    type?: string;
    url?: string;
  };
  bugs?: {
    url?: string;
  };
  homepage?: string;
}

export interface NpmVersionMetadata {
  name?: string;
  version?: string;
  description?: string;
  main?: string;
  scripts?: {
    [key: string]: string;
  };
  dependencies?: {
    [key: string]: string;
  };
  devDependencies?: {
    [key: string]: string;
  };
  keywords?: string[];
  license?: string;
  maintainers?: NpmMaintainer[];
  dist?: {
    tarball?: string;
    shasum?: string;
    integrity?: string;
  };
}

export interface NpmMaintainer {
  name?: string;
  email?: string;
}

export interface NpmPublishRequest {
  name?: string;
  version?: string;
  description?: string;
  main?: string;
  scripts?: {
    [key: string]: string;
  };
  dependencies?: {
    [key: string]: string;
  };
  devDependencies?: {
    [key: string]: string;
  };
  keywords?: string[];
  license?: string;
  maintainers?: NpmMaintainer[];
  repository?: {
    type?: string;
    url?: string;
  };
  bugs?: {
    url?: string;
  };
  homepage?: string;
}

export interface NpmPublishResponse {
  success?: boolean;
  id?: string;
  message?: string;
}

// ===== ESQUEMAS DE PYTHON =====
export interface PythonPackageUpload {
  name?: string;
  version?: string;
  filetype?: string;
  pyversion?: string;
  metadata_version?: string;
  summary?: string;
  home_page?: string;
  author?: string;
  author_email?: string;
  maintainer?: string;
  maintainer_email?: string;
  license?: string;
  description?: string;
  keywords?: string;
  platform?: string;
  classifiers?: string[];
  download_url?: string;
  requires_python?: string;
  requires_dist?: string[];
  provides_dist?: string[];
  obsoletes_dist?: string[];
  project_urls?: string[];
}

// ===== ESQUEMAS DE TOKENS/AUTH =====
export interface TokenRequest {
  name?: string;
  expiresAt?: string; // date-time
  scopes?: string[];
}

export interface TokenResponse {
  id?: string;
  name?: string;
  token?: string;
  expiresAt?: string; // date-time
  scopes?: string[];
  createdAt?: string; // date-time
}

export interface TokenInfo {
  id?: string;
  name?: string;
  expiresAt?: string; // date-time
  scopes?: string[];
  createdAt?: string; // date-time
  lastUsedAt?: string; // date-time
}

// ===== ESQUEMAS DE USERS =====
export interface CreateUserCommand {
  username?: string;
  email?: string;
  password?: string;
  attributes?: {
    [key: string]: any;
  };
}

export interface CreateUserResponse {
  id?: string;
  username?: string;
  email?: string;
  createdAt?: string; // date-time
}

export interface UpdateUserAttributesCommand {
  attributes?: {
    [key: string]: any;
  };
}

export interface UpdateUserAttributesResponse {
  id?: string;
  username?: string;
  email?: string;
  attributes?: {
    [key: string]: any;
  };
  updatedAt?: string; // date-time
}

// ===== ESQUEMAS DE POLICIES =====
export interface CreatePolicyCommand {
  name?: string;
  description?: string;
  policy?: string;
  isActive?: boolean;
}

export interface CreatePolicyResponse {
  id?: string;
  name?: string;
  description?: string;
  isActive?: boolean;
  createdAt?: string; // date-time
}

// ===== ESQUEMAS DE UPLOAD =====
export interface UploadResponse {
  id?: string;
  status?: 'accepted' | 'duplicate';
  message?: string;
  url?: string; // uri
}

// ===== ESQUEMAS DE ERROR =====
export interface Error {
  code?: string;
  message?: string;
  details?: any;
  timestamp?: string; // date-time
  path?: string;
  method?: string;
}

// ===== TIPOS DE PETICIÓN/RESPUESTA PARA CADA ENDPOINT =====

// GET /v1/repositories
export interface ListRepositoriesParams {
  limit?: number;
  offset?: number;
}

// POST /v1/repositories
export type CreateRepositoryBody = CreateRepositoryRequest;

// GET/PUT/DELETE /v1/repositories/{id}
export interface RepositoryParams {
  id: string; // uuid
}

// PUT /v1/repositories/{id}
export type UpdateRepositoryBody = UpdateRepositoryRequest;

// POST /v1/artifacts
export interface UploadArtifactBody {
  file: File;
  metadata: string; // JSON string
}

// GET /v1/artifacts/{id}
export interface GetArtifactParams {
  id: string;
  presigned?: boolean;
}

// GET /v1/search
export interface SearchArtifactsParams {
  q: string;
  limit?: number;
  offset?: number;
}

// GET /maven2/{groupId}/{artifactId}/{version}/{fileName}
export interface DownloadMavenParams {
  groupId: string;
  artifactId: string;
  version: string;
  fileName: string;
}

// PUT /maven2/{groupId}/{artifactId}/{version}/{fileName}
export type UploadMavenBody = Blob;

// GET /{packageName}
export interface GetNpmPackageParams {
  packageName: string;
}

// PUT /{packageName}
export type PublishNpmBody = NpmPublishRequest;

// GET /{packageName}/-/{fileName}
export interface DownloadNpmTarballParams {
  packageName: string;
  fileName: string;
}

// GET /simple/{packageName}/
export interface GetPypiSimpleParams {
  packageName: string;
}

// GET /packages/{fileName}
export interface DownloadPypiPackageParams {
  fileName: string;
}

// POST /packages
export type UploadPypiBody = FormData;

// GET /auth/tokens
export interface ListTokensParams {
  // No parameters
}

// POST /auth/tokens
export type CreateTokenBody = TokenRequest;

// GET/DELETE /auth/tokens/{tokenId}
export interface TokenParams {
  tokenId: string;
}

// GET /v1/users
export interface ListUsersParams {
  // No parameters
}

// GET /v1/users/{id}/attributes
export interface UserAttributesParams {
  id: string;
}

// PUT /v1/users/{id}/attributes
export type UpdateUserAttributesBody = UpdateUserAttributesCommand;

// GET /v1/policies
export interface ListPoliciesParams {
  // No parameters
}

// POST /v1/policies
export type CreatePolicyBody = CreatePolicyCommand;

// ===== ENUMS Y CONSTANTES =====
export const PackageType = {
  MAVEN: 'maven',
  NPM: 'npm',
  PYPI: 'pypi',
} as const;

export type PackageType = (typeof PackageType)[keyof typeof PackageType];

export const ArtifactStatus = {
  ACCEPTED: 'accepted',
  DUPLICATE: 'duplicate',
} as const;

export type ArtifactStatus =
  (typeof ArtifactStatus)[keyof typeof ArtifactStatus];

// ===== TIPOS DE UTILIDAD =====
export interface PaginatedResponse<T> {
  total: number;
  limit: number;
  offset: number;
  items: T[];
}

export interface ApiResponse<T> {
  data?: T;
  error?: Error;
  success: boolean;
}

export interface ApiError {
  error: Error;
  success: false;
}
