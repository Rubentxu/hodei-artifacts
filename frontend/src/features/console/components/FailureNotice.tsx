/**
 * What the console says when a request did not go well.
 *
 * The notice names the kind of failure and says what it means, because
 * the four answers the API gives are different and the person reading
 * them has to act differently: a permission that was refused is not a
 * thing that is missing, and what is stored cannot be read is neither.
 */

import { AlertTriangle, Lock, SearchX, ServerCrash, WifiOff } from 'lucide-react';

import type { Failure, FailureKind } from '../api/errors';

const TITLES: Record<FailureKind, string> = {
  unauthenticated: 'Your credential is not valid',
  permission: 'You are not allowed to do this',
  absence: 'There is nothing here',
  invalid: 'The request was not understood',
  conflict: 'It already exists',
  corruption: 'What is stored cannot be read',
  indeterminate: 'The decision could not be taken',
  unavailable: 'The service cannot answer right now',
};

const ICONS: Record<FailureKind, typeof Lock> = {
  unauthenticated: Lock,
  permission: Lock,
  absence: SearchX,
  invalid: AlertTriangle,
  conflict: AlertTriangle,
  corruption: ServerCrash,
  indeterminate: ServerCrash,
  unavailable: WifiOff,
};

export interface FailureNoticeProps {
  failure: Failure;
}

export const FailureNotice = ({ failure }: FailureNoticeProps) => {
  const Icon = ICONS[failure.kind];
  return (
    <div
      role="alert"
      className="flex items-start gap-3 rounded-md border border-red-200 bg-red-50 p-4"
    >
      <Icon className="mt-0.5 h-5 w-5 shrink-0 text-red-600" aria-hidden="true" />
      <div>
        <h3 className="text-sm font-semibold text-red-800">
          {TITLES[failure.kind]}
        </h3>
        <p className="mt-1 text-sm text-red-700">{failure.message}</p>
      </div>
    </div>
  );
};
