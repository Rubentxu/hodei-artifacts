/**
 * Operational view of the authenticated account's usage and quota limits.
 *
 * Usage and limits come exclusively from `GET /api/v1/account/usage`.
 * Unconfigured (`null`) limits are rendered explicitly as
 * "Unlimited / Not configured" rather than `0`, and errors or unavailable
 * states never fall back to `0` usage.
 */

import { HardDrive, RefreshCw, Upload } from 'lucide-react';

import { Button } from '@/components/ui/Button';
import { Card, CardContent } from '@/components/ui/Card';
import { Spinner } from '@/components/ui/Spinner';
import { formatSize } from '@/shared/utils/formatters';

import { failureOf } from '../api/errors';
import { useAccountUsage } from '../hooks/useConsole';
import { FailureNotice } from './FailureNotice';

/** Format a byte limit, distinguishing `null` (unconfigured) from `0`. */
const formatByteLimit = (limit: number | null): string =>
  limit === null ? 'Unlimited / Not configured' : formatSize(limit);

/** Format a count limit, distinguishing `null` (unconfigured) from `0`. */
const formatCountLimit = (limit: number | null): string =>
  limit === null ? 'Unlimited / Not configured' : String(limit);

export const AccountUsageView = () => {
  const usage = useAccountUsage();

  if (usage.isLoading) {
    return (
      <div className="flex items-center justify-center p-8" role="status">
        <Spinner />
        <span className="ml-2 text-sm text-gray-600">
          Loading account usage…
        </span>
      </div>
    );
  }

  if (usage.error || !usage.data) {
    return (
      <div className="flex flex-col gap-4">
        <FailureNotice failure={failureOf(usage.error)} />
        <div>
          <Button
            variant="outline"
            size="sm"
            onClick={() => {
              void usage.refetch();
            }}
            leftIcon={<RefreshCw />}
          >
            Try again
          </Button>
        </div>
      </div>
    );
  }

  const data = usage.data;
  const storageLimit = data.limits.max_stored_bytes_per_account;
  const uploadLimit = data.limits.max_concurrent_uploads_per_account;
  const storagePercent =
    storageLimit !== null && storageLimit > 0
      ? Math.min(100, Math.round((data.stored_bytes / storageLimit) * 100))
      : null;

  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-wrap items-center justify-between gap-4">
        <div className="text-sm text-gray-600">
          Account:{' '}
          <span className="font-semibold text-gray-900">{data.account}</span>
        </div>
        <Button
          variant="outline"
          size="sm"
          onClick={() => {
            void usage.refetch();
          }}
          leftIcon={<RefreshCw />}
        >
          Refresh
        </Button>
      </div>

      <div className="grid gap-6 md:grid-cols-2">
        <Card>
          <CardContent className="flex flex-col gap-4">
            <div className="flex items-center gap-2">
              <HardDrive className="h-5 w-5 text-blue-600" aria-hidden="true" />
              <h2 className="text-base font-semibold text-gray-900">Storage</h2>
            </div>

            <dl className="grid grid-cols-2 gap-4 text-sm">
              <div>
                <dt className="text-gray-500">Stored bytes</dt>
                <dd
                  data-testid="stored-bytes-value"
                  className="mt-1 text-lg font-semibold text-gray-900"
                >
                  {formatSize(data.stored_bytes)}
                </dd>
              </div>
              <div>
                <dt className="text-gray-500">In-flight bytes</dt>
                <dd
                  data-testid="in-flight-bytes-value"
                  className="mt-1 text-lg font-semibold text-gray-900"
                >
                  {formatSize(data.in_flight_bytes)}
                </dd>
              </div>
              <div className="col-span-2">
                <dt className="text-gray-500">Storage limit</dt>
                <dd
                  data-testid="storage-limit-value"
                  className="mt-1 font-medium text-gray-900"
                >
                  {formatByteLimit(storageLimit)}
                </dd>
              </div>
            </dl>

            {storagePercent !== null && (
              <div>
                <div className="mb-1 flex justify-between text-xs text-gray-500">
                  <span>Utilization</span>
                  <span>{storagePercent}%</span>
                </div>
                <div className="h-2 w-full overflow-hidden rounded-full bg-gray-100">
                  <div
                    className="h-full bg-blue-600 transition-all"
                    style={{ width: `${storagePercent}%` }}
                  />
                </div>
              </div>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardContent className="flex flex-col gap-4">
            <div className="flex items-center gap-2">
              <Upload className="h-5 w-5 text-purple-600" aria-hidden="true" />
              <h2 className="text-base font-semibold text-gray-900">Uploads</h2>
            </div>

            <dl className="grid grid-cols-2 gap-4 text-sm">
              <div>
                <dt className="text-gray-500">Active uploads</dt>
                <dd
                  data-testid="active-uploads-value"
                  className="mt-1 text-lg font-semibold text-gray-900"
                >
                  {data.active_uploads}
                </dd>
              </div>
              <div>
                <dt className="text-gray-500">Retry-After</dt>
                <dd className="mt-1 text-lg font-semibold text-gray-900">
                  {data.limits.retry_after_secs}s
                </dd>
              </div>
              <div className="col-span-2">
                <dt className="text-gray-500">Concurrent upload limit</dt>
                <dd
                  data-testid="upload-limit-value"
                  className="mt-1 font-medium text-gray-900"
                >
                  {formatCountLimit(uploadLimit)}
                </dd>
              </div>
            </dl>
          </CardContent>
        </Card>
      </div>

      {data.note && (
        <p className="rounded-md border border-gray-200 bg-gray-50 px-4 py-3 text-xs text-gray-600">
          {data.note}
        </p>
      )}
    </div>
  );
};
