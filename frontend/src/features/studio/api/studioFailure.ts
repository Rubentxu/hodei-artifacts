/**
 * What the studio says when a request did not go well.
 *
 * The studio tells apart the answers that the backend tells apart, and it
 * never flattens them into "something failed":
 *
 * - `denied` (403): the access decision. The administrative gate or the
 *   separation of editor and approver refused; nothing happened.
 * - `stale_head` (409): the head moved under the editor. It carries the
 *   head the store found.
 * - `rollback_to_head` (409): the target already decides.
 * - `too_large` (413): the body is over the limit of the studio.
 * - `not_implemented` (501): the lifecycle has no durable store.
 */

import { failureOf, type FailureKind } from '@/features/console/api/errors';

import { studioErrorOf } from './studioTypes';

/** The kinds of failure the studio tells apart. */
export type StudioFailureKind =
  | 'denied'
  | 'stale_head'
  | 'rollback_to_head'
  | 'history_full'
  | 'too_large'
  | 'not_implemented'
  | FailureKind;

/** A failure, with what the studio says about it. */
export interface StudioFailure {
  kind: StudioFailureKind;
  message: string;
  status?: number;
  /** Only on `stale_head`: the head the store actually has. */
  foundHead?: number;
}

/** The limit of a body of the studio, as the backend declares it. */
export const STUDIO_BODY_LIMIT_KIB = 512;

/** The failure of a request, as the studio tells it. */
export const studioFailureOf = (error: unknown): StudioFailure => {
  const failure = failureOf(error);
  const response = (error as { response?: { data?: unknown } } | null)
    ?.response;
  const body = studioErrorOf(response?.data);
  const status = failure.status;

  if (status === 403) {
    return {
      kind: 'denied',
      status,
      message: body?.message ?? failure.message,
    };
  }
  if (status === 409 && body?.error === 'stale_head') {
    return {
      kind: 'stale_head',
      status,
      message: body.message,
      foundHead: body.found_head,
    };
  }
  if (status === 409 && body?.error === 'rollback_to_head') {
    return { kind: 'rollback_to_head', status, message: body.message };
  }
  if (status === 409 && body?.error === 'history_full') {
    return { kind: 'history_full', status, message: body.message };
  }
  if (status === 413) {
    return {
      kind: 'too_large',
      status,
      message: `The request is larger than the ${STUDIO_BODY_LIMIT_KIB} KiB that the studio accepts.`,
    };
  }
  if (status === 501) {
    return {
      kind: 'not_implemented',
      status,
      message:
        body?.message ??
        'The policy lifecycle is not backed by a durable store.',
    };
  }
  if (status === 400 && body) {
    return { ...failure, message: body.message };
  }
  return failure;
};
