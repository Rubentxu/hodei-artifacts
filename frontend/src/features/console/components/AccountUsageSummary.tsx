/**
 * Compact account usage summary for the Overview / Dashboard.
 *
 * Reuses `useAccountUsage()` so it shares the same principal-scoped cache
 * as the full operational view and never infers usage from catalog items.
 */

import { Link } from 'react-router-dom';
import { HardDrive } from 'lucide-react';

import { Card, CardContent } from '@/components/ui/Card';
import { Spinner } from '@/components/ui/Spinner';
import { formatSize } from '@/shared/utils/formatters';

import { failureOf } from '../api/errors';
import { useAccountUsage } from '../hooks/useConsole';
import { FailureNotice } from './FailureNotice';

export const AccountUsageSummary = () => {
  const usage = useAccountUsage();

  return (
    <Card>
      <CardContent className="flex flex-col gap-4">
        <div className="flex items-center justify-between gap-2">
          <div className="flex items-center gap-2">
            <HardDrive className="h-5 w-5 text-blue-600" aria-hidden="true" />
            <h3 className="text-base font-semibold text-gray-900">
              Account Usage &amp; Quotas
            </h3>
          </div>
          <Link
            to="/console/usage"
            className="text-sm font-medium text-blue-700 hover:underline"
          >
            View details
          </Link>
        </div>

        {usage.isLoading ? (
          <div className="flex items-center py-2" role="status">
            <Spinner />
            <span className="ml-2 text-sm text-gray-600">
              Loading account usage…
            </span>
          </div>
        ) : usage.error || !usage.data ? (
          <FailureNotice failure={failureOf(usage.error)} />
        ) : (
          <dl className="grid grid-cols-1 gap-4 text-sm sm:grid-cols-3">
            <div>
              <dt className="text-gray-500">Account</dt>
              <dd className="mt-1 font-medium text-gray-900">
                {usage.data.account}
              </dd>
            </div>
            <div>
              <dt className="text-gray-500">Stored</dt>
              <dd className="mt-1 font-medium text-gray-900">
                {formatSize(usage.data.stored_bytes)} /{' '}
                {usage.data.limits.max_stored_bytes_per_account === null
                  ? 'Unlimited / Not configured'
                  : formatSize(usage.data.limits.max_stored_bytes_per_account)}
              </dd>
            </div>
            <div>
              <dt className="text-gray-500">Active uploads</dt>
              <dd className="mt-1 font-medium text-gray-900">
                {usage.data.active_uploads} /{' '}
                {usage.data.limits.max_concurrent_uploads_per_account ??
                  'Unlimited / Not configured'}
              </dd>
            </div>
          </dl>
        )}
      </CardContent>
    </Card>
  );
};
