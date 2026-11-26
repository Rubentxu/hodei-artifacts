/**
 * The access decision, as the studio shows it.
 *
 * A 403 is not an error of the engine and not an absence: somebody with
 * authority said no, and nothing happened. The panel looks different from
 * the other three answers of the studio (a decision of the policy, a gap
 * of evidence, an error of the engine) on purpose.
 */

import { Lock } from 'lucide-react';

export interface AccessDeniedProps {
  /** What the server said, when it said something. */
  detail?: string;
  /** Whether the whole studio is closed to the principal. */
  scope: 'studio' | 'action';
  testId?: string;
}

export const AccessDenied = ({
  detail,
  scope,
  testId = 'studio-denied',
}: AccessDeniedProps) => (
  <div
    role="alert"
    data-testid={testId}
    className="flex items-start gap-3 rounded-md border border-slate-400 bg-slate-100 p-4"
  >
    <Lock
      className="mt-0.5 h-5 w-5 shrink-0 text-slate-700"
      aria-hidden="true"
    />
    <div>
      <h3 className="text-sm font-semibold text-slate-900">Access denied</h3>
      <p className="mt-1 text-sm text-slate-800">
        {scope === 'studio'
          ? 'Your principal is not allowed to use the Policy Studio. Nothing was read or changed.'
          : 'Your principal is not allowed to do this. Nothing was changed.'}
      </p>
      {detail && detail !== 'You are not allowed to do this.' && (
        <p className="mt-1 text-xs text-slate-600">{detail}</p>
      )}
    </div>
  </div>
);
