/**
 * The translation of what the API answered into what the console says.
 *
 * The four answers that look alike from the outside — a permission that
 * was refused, a thing that is not there, what that cannot be read and
 * what could not be decided — have to arrive at the console as four
 * different things. A console that answers "nothing here" to all of them
 * is lying about three of them.
 */

import { describe, expect, it } from 'vitest';

import { failureOf, theReasonsOf } from '../../api/errors';

/** The error of a request that failed with a status and a body. */
const failed = (status: number, data: unknown): unknown => ({
  response: { status, data },
});

describe('failureOf', () => {
  it('tells a corrupt row apart from a decision that could not be taken', () => {
    const corrupt = failureOf(failed(500, { error: 'corruption' }));
    const indeterminate = failureOf(failed(500, { error: 'indeterminate' }));

    expect(corrupt.kind).toBe('corruption');
    expect(indeterminate.kind).toBe('indeterminate');
    expect(corrupt.message).not.toBe(indeterminate.message);
  });

  it('names the kind that the body declares', () => {
    expect(failureOf(failed(403, { error: 'permission' })).kind).toBe(
      'permission'
    );
    expect(failureOf(failed(404, { error: 'absence' })).kind).toBe('absence');
    expect(failureOf(failed(409, { error: 'conflict' })).kind).toBe('conflict');
    expect(failureOf(failed(401, { error: 'unauthenticated' })).kind).toBe(
      'unauthenticated'
    );
  });

  it('falls back to the status when the body does not name a kind', () => {
    expect(failureOf(failed(403, {})).kind).toBe('permission');
    expect(failureOf(failed(404, {})).kind).toBe('absence');
    expect(failureOf(failed(400, {})).kind).toBe('invalid');
    expect(failureOf(failed(503, {})).kind).toBe('unavailable');
  });

  it('says that it could not be decided when the status is a server error', () => {
    expect(failureOf(failed(500, {})).kind).toBe('indeterminate');
    expect(failureOf(failed(502, {})).kind).toBe('indeterminate');
  });

  it('falls back to the status when the body names a kind nobody knows', () => {
    expect(failureOf(failed(403, { error: 'teapot' })).kind).toBe('permission');
  });

  it('says that the service is not there when there was no answer', () => {
    expect(failureOf(new Error('the network is down')).kind).toBe('unavailable');
    expect(failureOf(undefined).kind).toBe('unavailable');
  });

  it('keeps the status of the answer', () => {
    expect(failureOf(failed(404, { error: 'absence' })).status).toBe(404);
  });

  it('does not name a resource that belongs to somebody else', () => {
    const absence = failureOf(failed(404, { error: 'absence' }));
    const permission = failureOf(failed(403, { error: 'permission' }));

    expect(absence.message).not.toMatch(/repository|account|principal/i);
    expect(permission.message).not.toMatch(/repository|account|principal/i);
  });

  it('tells a refusal of the trust policy apart from a request that was not understood', () => {
    const refused = failureOf(failed(422, { promoted: false, reasons: [] }));

    expect(refused.kind).toBe('refused');
    expect(refused.message).not.toBe(failureOf(failed(400, {})).message);
  });

  it('says that the service is not there when the deployment does not serve it', () => {
    expect(failureOf(failed(501, {})).kind).toBe('unavailable');
  });
});

describe('theReasonsOf', () => {
  it('gives the reasons of a refusal', () => {
    const reasons = theReasonsOf(
      failed(422, { promoted: false, reasons: ['the signature does not hold'] })
    );

    expect(reasons).toEqual(['the signature does not hold']);
  });

  it('has no reasons when the answer is not a refusal', () => {
    expect(theReasonsOf(failed(404, { error: 'absence' }))).toEqual([]);
    expect(theReasonsOf(new Error('the network is down'))).toEqual([]);
  });

  it('ignores the reasons that are not text', () => {
    expect(theReasonsOf(failed(422, { reasons: ['a reason', 7] }))).toEqual([
      'a reason',
    ]);
  });
});
