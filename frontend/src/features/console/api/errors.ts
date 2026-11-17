/**
 * What the API answered, translated into what the console says.
 *
 * The answers are different and the console has to tell them apart:
 *
 * - a **permission** that was refused: the resource exists, and this
 *   caller may not touch it.
 * - an **absence**: there is nothing there. The API answers the same way
 *   when the resource belongs to another account, and the console does
 *   not get to know which of the two it was.
 * - a **corruption**: what is stored cannot be read, and somebody has to
 *   fix it.
 * - an **indeterminacy**: the decision could not be taken. Nobody said
 *   no, and nobody said yes either.
 *
 * A console that answers "nothing here" to all four is lying about three
 * of them, and the lie is the one that costs the most: a page that says
 * "there is no package here" when what happened is that the catalog
 * could not be read sends somebody looking for a package that is there.
 */

/** The kinds of failure the API tells apart. */
export type FailureKind =
  | 'unauthenticated'
  | 'permission'
  | 'absence'
  | 'invalid'
  | 'conflict'
  | 'corruption'
  | 'indeterminate'
  | 'unavailable';

/** A failure, with what the console says about it. */
export interface Failure {
  kind: FailureKind;
  /** What the console says. It never names a resource of another account. */
  message: string;
  /** The status the API answered with, when there was one. */
  status?: number;
}

/** What the console says about each kind of failure. */
const MESSAGES: Record<FailureKind, string> = {
  unauthenticated: 'Your credential is not valid. Sign in again.',
  permission: 'You are not allowed to do this.',
  absence: 'There is nothing here.',
  invalid: 'The request was not understood.',
  conflict: 'It already exists.',
  corruption: 'What is stored cannot be read.',
  indeterminate: 'The decision could not be taken.',
  unavailable: 'The service cannot answer right now.',
};

/** The kind of a status, when the body does not say one. */
const BY_STATUS: Record<number, FailureKind> = {
  400: 'invalid',
  401: 'unauthenticated',
  403: 'permission',
  404: 'absence',
  409: 'conflict',
  422: 'invalid',
  500: 'indeterminate',
  502: 'indeterminate',
  503: 'unavailable',
  504: 'unavailable',
};

/** Whether the body carries a kind the console knows. */
const isKnownKind = (value: unknown): value is FailureKind =>
  typeof value === 'string' && value in MESSAGES;

/**
 * The failure of a request.
 *
 * The body of the API says the kind when it can: a corrupt row and a
 * decision that could not be taken are both a 500, and the status alone
 * cannot tell them apart. The status is the fallback, and the fallback
 * of a 500 is the indeterminacy: what the API could not decide is what
 * the console cannot either.
 */
export const failureOf = (error: unknown): Failure => {
  const response = theResponse(error);
  if (!response) {
    return {
      kind: 'unavailable',
      message: MESSAGES.unavailable,
    };
  }
  const status = response.status;
  const kind = theKindOf(response.data) ?? BY_STATUS[status] ?? 'indeterminate';
  return {
    kind,
    message: MESSAGES[kind],
    status,
  };
};

/** The kind that the body declares, when it declares one. */
const theKindOf = (data: unknown): FailureKind | undefined => {
  if (typeof data !== 'object' || data === null) return undefined;
  const body = data as { error?: unknown; kind?: unknown };
  if (isKnownKind(body.kind)) return body.kind;
  if (isKnownKind(body.error)) return body.error;
  return undefined;
};

/** The response of a failed request, when there was one. */
interface FailedResponse {
  status: number;
  data: unknown;
}

/** The response of an error, without assuming it is an axios error. */
const theResponse = (error: unknown): FailedResponse | undefined => {
  if (typeof error !== 'object' || error === null) return undefined;
  const candidate = error as {
    response?: { status?: unknown; data?: unknown };
    isAxiosError?: boolean;
  };
  const response = candidate.response;
  if (!response || typeof response.status !== 'number') return undefined;
  return { status: response.status, data: response.data };
};
