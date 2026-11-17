/**
 * The hooks of the console.
 *
 * They ask the API and they say what happened. The failure of a request
 * is kept as a failure —with its kind— and not flattened into a
 * message: the console has to be able to tell a permission that was
 * refused from a thing that is not there.
 */

import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import * as api from '../api/consoleApi';
import type { SearchParams } from '../api/consoleApi';

const KEYS = {
  /** Everything the console asks for. */
  root: ['console'],
  repositories: ['console', 'repositories'],
  capabilities: ['console', 'capabilities'],
  search: (params: SearchParams) => ['console', 'search', params],
  package: (repository: string, path: string) => [
    'console',
    'package',
    repository,
    path,
  ],
} as const;

/** The repositories of the account that the principal may read. */
export const useRepositories = () =>
  useQuery({
    queryKey: KEYS.repositories,
    queryFn: api.listRepositories,
  });

/**
 * What can be created.
 *
 * It is asked once and kept: the answer is a fact of the deployment, and
 * it does not change while the console is open.
 */
export const useCapabilities = () =>
  useQuery({
    queryKey: KEYS.capabilities,
    queryFn: api.capabilities,
    staleTime: 300_000,
  });

/** A page of the search of the catalog. */
export const useSearch = (params: SearchParams, enabled = true) =>
  useQuery({
    queryKey: KEYS.search(params),
    queryFn: () => api.search(params),
    enabled,
  });

/** The detail of a package. */
export const usePackage = (repository: string, path: string) =>
  useQuery({
    queryKey: KEYS.package(repository, path),
    queryFn: () => api.packageDetail(repository, path),
    enabled: repository.length > 0 && path.length > 0,
  });

/** The creation of a repository of the catalog. */
export const useCreateRepository = () => {
  const client = useQueryClient();
  return useMutation({
    mutationFn: api.createRepository,
    onSuccess: () => {
      client.invalidateQueries({ queryKey: KEYS.repositories });
    },
  });
};

/** The promotion of a content to the trusted channel. */
export const usePromote = () => {
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({
      repository,
      path,
      digest,
    }: {
      repository: string;
      path: string;
      digest: string;
    }) => api.promote(repository, path, digest),
    onSuccess: () => {
      // The console's own queries, and not the whole application: a
      // promotion does not change what the rest of the frontend asks
      // for.
      client.invalidateQueries({ queryKey: KEYS.root });
    },
  });
};

/** The revocation of a concession. */
export const useRevoke = () => {
  const client = useQueryClient();
  return useMutation({
    mutationFn: api.revoke,
    onSuccess: () => {
      // The console's own queries, and not the whole application: a
      // revocation does not change what the rest of the frontend asks
      // for.
      client.invalidateQueries({ queryKey: KEYS.root });
    },
  });
};
