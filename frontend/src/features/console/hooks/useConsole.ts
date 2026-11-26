/**
 * The hooks of the console.
 *
 * They ask the API and they say what happened. The failure of a request
 * is kept as a failure —with its kind— and not flattened into a
 * message: the console has to be able to tell a permission that was
 * refused from a thing that is not there.
 */

import { useEffect, useRef } from 'react';
import axios from 'axios';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import {
  selectPrincipalSessionKey,
  useAuthStore,
} from '@/shared/stores/auth.store';
import * as api from '../api/consoleApi';
import type { RevokeBody, SearchParams } from '../api/consoleApi';
import type { CreateRepositoryBody } from '../api/types';

const KEYS = {
  /** Everything the console asks for, scoped by principal session. */
  root: (session: string) => ['console', session] as const,
  repositories: (session: string) =>
    ['console', session, 'repositories'] as const,
  capabilities: (session: string) =>
    ['console', session, 'capabilities'] as const,
  accountUsage: (session: string) =>
    ['console', session, 'account-usage'] as const,
  search: (session: string, params: SearchParams) =>
    ['console', session, 'search', params] as const,
  package: (session: string, repository: string, path: string) =>
    ['console', session, 'package', repository, path] as const,
};

/**
 * Tracks the active principal session and cancels/evicts queries from any
 * previous session as soon as logout or principal switch occurs.
 */
const useConsoleSessionGuard = (): string => {
  const client = useQueryClient();
  const session = useAuthStore(selectPrincipalSessionKey);
  const prevSessionRef = useRef(session);

  useEffect(() => {
    const prevSession = prevSessionRef.current;
    if (prevSession !== session) {
      prevSessionRef.current = session;
      void client.cancelQueries({ queryKey: KEYS.root(prevSession) });
      client.removeQueries({ queryKey: KEYS.root(prevSession) });
    }
  }, [client, session]);

  return session;
};

/**
 * Resets mutation state and aborts in-flight work if the session changes.
 */
const useSessionMutationGuard = (reset: () => void) => {
  const session = useConsoleSessionGuard();
  const prevSessionRef = useRef(session);
  const resetRef = useRef(reset);
  resetRef.current = reset;

  useEffect(() => {
    if (prevSessionRef.current !== session) {
      prevSessionRef.current = session;
      resetRef.current();
    }
  }, [session]);

  return session;
};

const runGuardedMutation = async <T>(
  execute: (signal: AbortSignal) => Promise<T>
): Promise<T> => {
  const startEpoch = useAuthStore.getState().sessionEpoch;
  const controller = new AbortController();

  const unsubscribe = useAuthStore.subscribe(nextState => {
    if (nextState.sessionEpoch !== startEpoch) {
      controller.abort();
    }
  });

  try {
    const data = await execute(controller.signal);
    if (useAuthStore.getState().sessionEpoch !== startEpoch) {
      throw new axios.CanceledError(
        'Session changed while mutation was in flight'
      );
    }
    return data;
  } finally {
    unsubscribe();
  }
};

const captureMutationSession = () => {
  const state = useAuthStore.getState();
  return {
    epoch: state.sessionEpoch,
    session: selectPrincipalSessionKey(state),
  };
};

/** The repositories of the account that the principal may read. */
export const useRepositories = () => {
  const session = useConsoleSessionGuard();
  return useQuery({
    queryKey: KEYS.repositories(session),
    queryFn: ({ signal }) => api.listRepositories(signal),
  });
};

/**
 * The usage and quota limits of the authenticated principal's own account.
 *
 * Scoped by the active principal/session so switching accounts never leaks
 * cached usage data from a previous principal.
 */
export const useAccountUsage = () => {
  const session = useConsoleSessionGuard();
  return useQuery({
    queryKey: KEYS.accountUsage(session),
    queryFn: ({ signal }) => api.accountUsage(signal),
  });
};

/**
 * What can be created.
 *
 * It is asked once and kept: the answer is a fact of the deployment, and
 * it does not change while the console is open.
 */
export const useCapabilities = (enabled = true) => {
  const session = useConsoleSessionGuard();
  return useQuery({
    queryKey: KEYS.capabilities(session),
    queryFn: ({ signal }) => api.capabilities(signal),
    staleTime: 300_000,
    enabled,
  });
};

/** A page of the search of the catalog. */
export const useSearch = (params: SearchParams, enabled = true) => {
  const session = useConsoleSessionGuard();
  return useQuery({
    queryKey: KEYS.search(session, params),
    queryFn: ({ signal }) => api.search(params, signal),
    enabled,
  });
};

/** The detail of a package. */
export const usePackage = (repository: string, path: string) => {
  const session = useConsoleSessionGuard();
  return useQuery({
    queryKey: KEYS.package(session, repository, path),
    queryFn: ({ signal }) => api.packageDetail(repository, path, signal),
    enabled: repository.length > 0 && path.length > 0,
  });
};

/** The creation of a repository of the catalog. */
export const useCreateRepository = () => {
  const client = useQueryClient();
  const mutation = useMutation({
    onMutate: captureMutationSession,
    mutationFn: (body: CreateRepositoryBody) =>
      runGuardedMutation(signal => api.createRepository(body, signal)),
    onSuccess: (_data, _variables, context) => {
      if (context && useAuthStore.getState().sessionEpoch === context.epoch) {
        void client.invalidateQueries({
          queryKey: KEYS.repositories(context.session),
        });
      }
    },
  });
  useSessionMutationGuard(mutation.reset);
  return mutation;
};

/** The promotion of a content to the trusted channel. */
export const usePromote = () => {
  const client = useQueryClient();
  const mutation = useMutation({
    onMutate: captureMutationSession,
    mutationFn: ({
      repository,
      path,
      digest,
    }: {
      repository: string;
      path: string;
      digest: string;
    }) =>
      runGuardedMutation(signal =>
        api.promote(repository, path, digest, signal)
      ),
    onSuccess: (_data, _variables, context) => {
      // Invalidate only the active principal's queries and keep the mutation
      // pending until the refreshed trust state has been fetched.
      if (context && useAuthStore.getState().sessionEpoch === context.epoch) {
        return client.invalidateQueries({
          queryKey: KEYS.root(context.session),
        });
      }
    },
  });
  useSessionMutationGuard(mutation.reset);
  return mutation;
};

/** The revocation of a concession. */
export const useRevoke = () => {
  const client = useQueryClient();
  const mutation = useMutation({
    onMutate: captureMutationSession,
    mutationFn: (body: RevokeBody) =>
      runGuardedMutation(signal => api.revoke(body, signal)),
    onSuccess: (_data, _variables, context) => {
      // Keep the mutation pending until the active session sees its refreshed
      // trust state; never invalidate queries belonging to another principal.
      if (context && useAuthStore.getState().sessionEpoch === context.epoch) {
        return client.invalidateQueries({
          queryKey: KEYS.root(context.session),
        });
      }
    },
  });
  useSessionMutationGuard(mutation.reset);
  return mutation;
};

/** A read-only investigation of an artefact. */
export const useInvestigate = () =>
  useMutation({
    mutationFn: api.investigate,
  });

/** On-demand freshness check for an existing investigation report. */
export const useInvestigateFreshness = () =>
  useMutation({
    mutationFn: api.investigateFreshness,
  });
