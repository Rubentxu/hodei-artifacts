/**
 * The client of the console.
 *
 * It talks to the real API: there is no mock behind it and no adapter
 * that renames what the API says. The credential travels in the header,
 * the account is the one of the verified principal, and what comes back
 * is what the API answered.
 *
 * The paths are relative on purpose: the development server proxies
 * `/api` to the service, and the service that serves the console is the
 * same one that serves the API.
 */

import axios, { type AxiosInstance } from 'axios';

import { useAuthStore } from '@/shared/stores/auth.store';
import type {
  Capabilities,
  CreateRepositoryBody,
  PackageView,
  PromotionResult,
  RepositoryList,
  RepositorySummary,
  SearchPage,
} from './types';

/** The filters of the search, as the client writes them. */
export interface SearchParams {
  repository?: string;
  package?: string;
  version?: string;
  digest?: string;
  tag?: string;
  attribute?: string;
  text?: string;
  limit?: number;
  offset?: number;
}

/** The body of a revocation. */
export interface RevokeBody {
  repository: string;
  digest?: string;
  decision_id?: string;
  reason: string;
}

const client: AxiosInstance = axios.create({
  baseURL: '/api/v1',
  timeout: 30000,
  headers: { 'Content-Type': 'application/json' },
});

client.interceptors.request.use(config => {
  const { token } = useAuthStore.getState();
  if (token) {
    config.headers.Authorization = `Bearer ${token}`;
  }
  return config;
});

/**
 * The client of the content surface.
 *
 * The content is not under `/api/v1`: it is served where the protocol
 * says it is, at the root of the service. The credential travels the
 * same way, because a download that needs a link with the token in it
 * would put the token in a place where it ends up in the history of
 * somebody's browser.
 */
const content: AxiosInstance = axios.create({
  baseURL: '/',
  timeout: 120000,
});

content.interceptors.request.use(config => {
  const { token } = useAuthStore.getState();
  if (token) {
    config.headers.Authorization = `Bearer ${token}`;
  }
  return config;
});

/** The repositories of the account that the principal may read. */
export const listRepositories = async (): Promise<RepositoryList> => {
  const response = await client.get<RepositoryList>('/catalog/repositories');
  return response.data;
};

/** What can be created. */
export const capabilities = async (): Promise<Capabilities> => {
  const response = await client.get<Capabilities>('/capabilities');
  return response.data;
};

/** The creation of a repository of the catalog. */
export const createRepository = async (
  body: CreateRepositoryBody
): Promise<RepositorySummary> => {
  const response = await client.post<RepositorySummary>(
    '/catalog/repositories',
    body
  );
  return response.data;
};

/** A page of the search of the catalog. */
export const search = async (
  params: SearchParams
): Promise<SearchPage> => {
  const response = await client.get<SearchPage>('/search', { params });
  return response.data;
};

/** The detail of a package. */
export const packageDetail = async (
  repository: string,
  path: string
): Promise<PackageView> => {
  const response = await client.get<PackageView>(
    `/packages/${encodeURIComponent(repository)}/${thePathOf(path)}`
  );
  return response.data;
};

/** The promotion of a content to the trusted channel. */
export const promote = async (
  repository: string,
  path: string,
  digest: string
): Promise<PromotionResult> => {
  const response = await client.post<PromotionResult>('/promotions', {
    repository,
    path,
    digest,
  });
  return response.data;
};

/** The revocation of a concession. */
export const revoke = async (body: RevokeBody): Promise<unknown> => {
  const response = await client.post('/promotions/revoke', body);
  return response.data;
};

/**
 * The bytes of an asset.
 *
 * The content is served by the content surface and not by the API, and it
 * needs the credential: the console asks for it and hands it to the
 * browser, instead of building a link with the token inside it.
 */
export const downloadAsset = async (
  repository: string,
  path: string
): Promise<Blob> => {
  const response = await content.get(
    `/repository/${encodeURIComponent(repository)}/${thePathOf(path)}`,
    { responseType: 'blob' }
  );
  return response.data as Blob;
};

/**
 * The path of an asset, with its slashes.
 *
 * The route is a wildcard: the path names the content inside the
 * repository, and the slashes are part of that name. Each segment is
 * escaped on its own, so a segment cannot escape its own place.
 */
const thePathOf = (path: string): string =>
  path
    .split('/')
    .filter(segment => segment.length > 0)
    .map(segment => encodeURIComponent(segment))
    .join('/');
