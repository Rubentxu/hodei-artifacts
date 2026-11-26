/**
 * The client of the Policy Studio.
 *
 * It talks to the server-side surface of `/api/v1/policy-studio`: the same
 * lifecycle the promotions read, reached through the administrative gate.
 * Nothing is decided in the browser: there is no Cedar, no trust engine
 * and no local simulation here. What the browser shows is what the
 * server measured.
 *
 * The credential travels in the header like in the rest of the console,
 * and the account, the editor and the approver are the ones of the
 * verified token: none of them is a parameter of any call below.
 */

import axios, { type AxiosInstance } from 'axios';

import { attachSessionGuard } from '@/features/console/api/consoleApi';

import type {
  ActivationResponse,
  HeadResponse,
  HistoryResponse,
  RevisionResponse,
  SaveResponse,
  SimulatedSubject,
  SimulateResponse,
  StudioPolicy,
  ValidateResponse,
} from './studioTypes';

const client: AxiosInstance = axios.create({
  baseURL: '/api/v1/policy-studio',
  timeout: 30000,
  headers: { 'Content-Type': 'application/json' },
});
attachSessionGuard(client);

/** The head that the next activation compares against. */
export const loadHead = async (signal?: AbortSignal): Promise<HeadResponse> => {
  const { data } = await client.get<HeadResponse>('/head', { signal });
  return data;
};

/** A revision: the one that decides, or the one named. */
export const loadRevision = async (
  sequence?: number,
  signal?: AbortSignal
): Promise<RevisionResponse> => {
  const { data } = await client.get<RevisionResponse>('/revisions', {
    params: sequence === undefined ? undefined : { sequence },
    signal,
  });
  return data;
};

/** A window of the history: revisions and events share the offset. */
export const loadHistory = async (
  offset: number,
  limit: number,
  signal?: AbortSignal
): Promise<HistoryResponse> => {
  const { data } = await client.get<HistoryResponse>('/history', {
    params: { offset, limit },
    signal,
  });
  return data;
};

/** The strict validation of a draft. It saves nothing. */
export const validateDraft = async (
  policy: StudioPolicy
): Promise<ValidateResponse> => {
  const { data } = await client.post<ValidateResponse>('/validate', {
    policy,
  });
  return data;
};

/** Saves a draft as a new immutable revision. Saving never activates. */
export const saveRevision = async (
  policy: StudioPolicy,
  reason: string
): Promise<SaveResponse> => {
  const { data } = await client.post<SaveResponse>('/revisions', {
    policy,
    reason,
  });
  return data;
};

/** Measures a draft against evidence snapshots. It changes nothing. */
export const simulateDraft = async (
  policy: StudioPolicy,
  subjects: SimulatedSubject[]
): Promise<SimulateResponse> => {
  const { data } = await client.post<SimulateResponse>('/simulate', {
    policy,
    subjects,
  });
  return data;
};

/**
 * Activates a **saved** revision through the compare-and-set of the store.
 *
 * It carries the head the editor saw. If the head moved, the store
 * answers `409 stale_head` with the head it found.
 */
export const activateRevision = async (
  sequence: number,
  expectedHead: number | null,
  reason: string
): Promise<ActivationResponse> => {
  const { data } = await client.post<ActivationResponse>('/activate', {
    sequence,
    expected_head: expectedHead,
    reason,
  });
  return data;
};

/** Rolls back to a revision, as a new revision, with the same compare-and-set. */
export const rollbackTo = async (
  target: number,
  expectedHead: number | null,
  reason: string
): Promise<ActivationResponse> => {
  const { data } = await client.post<ActivationResponse>('/rollback', {
    target,
    expected_head: expectedHead,
    reason,
  });
  return data;
};
