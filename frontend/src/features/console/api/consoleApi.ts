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

import axios, {
  type AxiosInstance,
  type InternalAxiosRequestConfig,
} from 'axios';

import { useAuthStore } from '@/shared/stores/auth.store';
import { encodeAssetPath } from '../lib/packageReference';
import type {
  AccountUsage,
  Capabilities,
  CreateRepositoryBody,
  FreshnessRequest,
  FreshnessResponse,
  InvestigationReport,
  InvestigationRequest,
  PackageView,
  PromotionResult,
  RepositoryList,
  RepositorySummary,
  RevokeResult,
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

type SessionGuardedConfig = InternalAxiosRequestConfig & {
  _sessionEpoch?: number;
};

/** Binds a client to the session: the credential and the epoch guard. */
export const attachSessionGuard = (instance: AxiosInstance): void => {
  instance.interceptors.request.use(config => {
    const guarded = config as SessionGuardedConfig;
    const { token, sessionEpoch } = useAuthStore.getState();
    guarded._sessionEpoch = sessionEpoch;
    if (token) {
      guarded.headers.Authorization = `Bearer ${token}`;
    }
    return guarded;
  });

  instance.interceptors.response.use(
    response => {
      const startEpoch = (response.config as SessionGuardedConfig)
        ._sessionEpoch;
      if (
        startEpoch !== undefined &&
        startEpoch !== useAuthStore.getState().sessionEpoch
      ) {
        return Promise.reject(
          new axios.CanceledError('Session changed while request was in flight')
        );
      }
      return response;
    },
    (error: unknown) => {
      const config = (error as { config?: SessionGuardedConfig } | null)
        ?.config;
      const startEpoch = config?._sessionEpoch;
      if (
        startEpoch !== undefined &&
        startEpoch !== useAuthStore.getState().sessionEpoch
      ) {
        return Promise.reject(
          new axios.CanceledError('Session changed while request was in flight')
        );
      }
      return Promise.reject(error);
    }
  );
};

const client: AxiosInstance = axios.create({
  baseURL: '/api/v1',
  timeout: 30000,
  headers: { 'Content-Type': 'application/json' },
});
attachSessionGuard(client);

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
attachSessionGuard(content);

/** The repositories of the account that the principal may read. */
export const listRepositories = async (
  signal?: AbortSignal
): Promise<RepositoryList> => {
  const response = await client.get<RepositoryList>('/catalog/repositories', {
    signal,
  });
  return response.data;
};

/** What can be created. */
export const capabilities = async (
  signal?: AbortSignal
): Promise<Capabilities> => {
  const response = await client.get<Capabilities>('/capabilities', { signal });
  return response.data;
};

/**
 * The usage and quota limits of the authenticated principal's own account.
 *
 * There is no account parameter: the server derives the account from the
 * verified credential in the Authorization header.
 */
export const accountUsage = async (
  signal?: AbortSignal
): Promise<AccountUsage> => {
  const response = await client.get<AccountUsage>('/account/usage', { signal });
  return response.data;
};

/** The creation of a repository of the catalog. */
export const createRepository = async (
  body: CreateRepositoryBody,
  signal?: AbortSignal
): Promise<RepositorySummary> => {
  const response = await client.post<RepositorySummary>(
    '/catalog/repositories',
    body,
    { signal }
  );
  return response.data;
};

/** A page of the search of the catalog. */
export const search = async (
  params: SearchParams,
  signal?: AbortSignal
): Promise<SearchPage> => {
  const response = await client.get<SearchPage>('/search', { params, signal });
  return response.data;
};

/** The detail of a package. */
export const packageDetail = async (
  repository: string,
  path: string,
  signal?: AbortSignal
): Promise<PackageView> => {
  const response = await client.get<PackageView>(
    `/packages/${encodeURIComponent(repository)}/${encodeAssetPath(path)}`,
    { signal }
  );
  return response.data;
};

/** The promotion of a content to the trusted channel. */
export const promote = async (
  repository: string,
  path: string,
  digest: string,
  signal?: AbortSignal
): Promise<PromotionResult> => {
  const response = await client.post<PromotionResult>(
    '/promotions',
    {
      repository,
      path,
      digest,
    },
    { signal }
  );
  return response.data;
};

/** The revocation of a concession. */
export const revoke = async (
  body: RevokeBody,
  signal?: AbortSignal
): Promise<RevokeResult> => {
  const response = await client.post<RevokeResult>('/promotions/revoke', body, {
    signal,
  });
  return response.data;
};

/**
 * A read-only investigation of an artefact.
 *
 * The subject is the canonical HRN provided by the service, and the
 * question is optional and bounded. Nothing is written or promoted.
 */
export const investigate = async (
  body: InvestigationRequest
): Promise<InvestigationReport> => {
  const response = await client.post<InvestigationReport>(
    '/investigations',
    body
  );
  return response.data;
};

/**
 * On-demand freshness verification of an investigation report.
 *
 * Sends the report's original `subject` and `read_set` verbatim without
 * mutating or reconstructing them. Nothing is written or invalidated.
 */
export const investigateFreshness = async (
  body: FreshnessRequest
): Promise<FreshnessResponse> => {
  const response = await client.post<FreshnessResponse>(
    '/investigations/freshness',
    body
  );
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
  path: string,
  signal?: AbortSignal
): Promise<Blob> => {
  const response = await content.get(
    `/repository/${encodeURIComponent(repository)}/${encodeAssetPath(path)}`,
    { responseType: 'blob', signal }
  );
  return response.data as Blob;
};
