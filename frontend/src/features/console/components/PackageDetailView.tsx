/**
 * The detail of an asset.
 *
 * It says what the asset is: the digest that names the bytes, how many
 * bytes there are, the version it was published as, where it came from,
 * what the evidence says about it and what has been decided about it.
 *
 * The trust comes from the grant, not from the absence of findings:
 * content nobody has ever written about is quarantined, and the view
 * says so. The decision that is shown is the live one, because a revoked
 * grant is not a grant.
 *
 * The two actions that change what the service serves —the promotion and
 * the revocation— ask before they do it, and the download asks for the
 * bytes from the content surface with the credential of the person, and
 * not from a link with the credential inside it.
 */

import { useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import {
  AlertTriangle,
  Check,
  Copy,
  Download,
  Link2,
  ShieldCheck,
  ShieldOff,
} from 'lucide-react';

import { Badge } from '@/components/ui/Badge';
import { Button } from '@/components/ui/Button';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/Card';
import { Input } from '@/components/ui/Input';
import { Spinner } from '@/components/ui/Spinner';

import { failureOf, theReasonsOf } from '../api/errors';
import { downloadAsset } from '../api/consoleApi';
import type { TrustState } from '../api/types';
import { usePackage, usePromote, useRevoke } from '../hooks/useConsole';
import {
  buildCanonicalReference,
  buildShareablePackageUrl,
  digestsMatch,
  formatDigest,
} from '../lib/packageReference';
import { ConfirmDialog } from './ConfirmDialog';
import { FailureNotice } from './FailureNotice';

/** The label of a trust state. */
const TRUST_LABELS: Record<TrustState, string> = {
  quarantined: 'Quarantined',
  trusted: 'Trusted',
};

/** The badge of a trust state. */
const TRUST_VARIANT: Record<TrustState, 'warning' | 'success'> = {
  quarantined: 'warning',
  trusted: 'success',
};

/** The bytes, in the unit that reads well. */
const theSize = (bytes: number): string => {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
};

/** The moment, as the console says it. */
const theMoment = (value: string): string => {
  const moment = new Date(value);
  return Number.isNaN(moment.getTime()) ? value : moment.toISOString();
};

export interface PackageDetailViewProps {
  repository: string;
  path: string;
  /** Optional expected digest override (defaults to `?digest=` query param). */
  expectedDigest?: string;
}

export const PackageDetailView = ({
  repository,
  path,
  expectedDigest,
}: PackageDetailViewProps) => {
  const [searchParams] = useSearchParams();
  const detail = usePackage(repository, path);
  const promote = usePromote();
  const revoke = useRevoke();
  const [promoting, setPromoting] = useState(false);
  const [revoking, setRevoking] = useState(false);
  const [reason, setReason] = useState('');
  const [downloading, setDownloading] = useState(false);
  const [downloadFailure, setDownloadFailure] = useState<unknown>(null);
  const [copiedKey, setCopiedKey] = useState<
    'digest' | 'reference' | 'link' | null
  >(null);

  const pinnedDigest = expectedDigest ?? searchParams.get('digest') ?? '';

  if (detail.isLoading) {
    return (
      <div className="flex items-center justify-center p-8" role="status">
        <Spinner />
        <span className="ml-2 text-sm text-gray-600">Loading the asset…</span>
      </div>
    );
  }

  if (detail.error) {
    return <FailureNotice failure={failureOf(detail.error)} />;
  }

  const asset = detail.data;
  if (!asset) return null;

  const digest = formatDigest(asset.digest);
  const blobDigest = formatDigest(asset.blob.digest);
  const publishedDigest = formatDigest(asset.published.digest);
  const hasDigestMismatch =
    pinnedDigest.trim().length > 0 && !digestsMatch(pinnedDigest, asset.digest);
  const canonicalReference = buildCanonicalReference(
    asset.repository,
    asset.asset_path
  );
  const shareableUrl = buildShareablePackageUrl(
    window.location.origin,
    asset.repository,
    asset.asset_path,
    digest
  );

  const copyText = async (
    key: 'digest' | 'reference' | 'link',
    value: string
  ) => {
    try {
      await navigator.clipboard.writeText(value);
      setCopiedKey(key);
      setTimeout(() => {
        setCopiedKey(current => (current === key ? null : current));
      }, 2000);
    } catch {
      setCopiedKey(null);
    }
  };

  const download = async () => {
    setDownloading(true);
    setDownloadFailure(null);
    try {
      const blob = await downloadAsset(asset.repository, asset.asset_path);
      const url = URL.createObjectURL(blob);
      const anchor = document.createElement('a');
      anchor.href = url;
      anchor.download = asset.asset_path.split('/').pop() ?? 'asset';
      document.body.appendChild(anchor);
      anchor.click();
      anchor.remove();
      URL.revokeObjectURL(url);
    } catch (error) {
      setDownloadFailure(error);
    } finally {
      setDownloading(false);
    }
  };

  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 className="text-xl font-semibold text-gray-900">
            {asset.package_name || asset.asset_path}
          </h2>
          <p className="text-sm text-gray-600">
            {asset.repository} · {asset.asset_path}
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Badge variant={TRUST_VARIANT[asset.trust.state]}>
            {TRUST_LABELS[asset.trust.state]}
          </Badge>
          {/* While the detail is being asked again, what is on the
              screen is what was true a moment ago, and the console says
              so instead of letting it look like the truth. */}
          {detail.isFetching && (
            <span
              className="flex items-center gap-2 text-xs text-gray-500"
              role="status"
            >
              <Spinner />
              Updating…
            </span>
          )}
          <Button
            variant="outline"
            onClick={download}
            isLoading={downloading}
            leftIcon={<Download />}
          >
            Download
          </Button>
          {asset.trust.state === 'trusted' ? (
            <Button
              variant="destructive"
              onClick={() => setRevoking(true)}
              leftIcon={<ShieldOff />}
            >
              Revoke
            </Button>
          ) : (
            <Button
              onClick={() => setPromoting(true)}
              leftIcon={<ShieldCheck />}
            >
              Promote
            </Button>
          )}
        </div>
      </div>

      {downloadFailure !== null && (
        <FailureNotice failure={failureOf(downloadFailure)} />
      )}

      {hasDigestMismatch && (
        <div
          role="alert"
          className="flex items-start gap-3 rounded-lg border border-amber-300 bg-amber-50 p-4 text-amber-950"
        >
          <AlertTriangle className="mt-0.5 h-5 w-5 shrink-0 text-amber-600" />
          <div className="flex flex-col gap-1 text-sm">
            <p className="font-semibold">
              Content changed: this path no longer matches the expected digest
            </p>
            <p className="text-amber-900">
              Repository paths are mutable and can be updated to point to new
              bytes. Verify the digest before trusting or downloading this
              artifact.
            </p>
            <dl className="mt-1 grid gap-1 text-xs">
              <div className="flex flex-wrap gap-2">
                <dt className="font-medium">Expected digest:</dt>
                <dd className="break-all font-mono">{pinnedDigest.trim()}</dd>
              </div>
              <div className="flex flex-wrap gap-2">
                <dt className="font-medium">Current digest:</dt>
                <dd className="break-all font-mono">{digest}</dd>
              </div>
            </dl>
          </div>
        </div>
      )}

      <Card>
        <CardHeader className="flex flex-row flex-wrap items-center justify-between gap-2">
          <CardTitle>Identity</CardTitle>
          <div className="flex flex-wrap items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => copyText('digest', digest)}
              leftIcon={copiedKey === 'digest' ? <Check /> : <Copy />}
            >
              {copiedKey === 'digest' ? 'Copied digest' : 'Copy digest'}
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={() => copyText('reference', canonicalReference)}
              leftIcon={copiedKey === 'reference' ? <Check /> : <Copy />}
            >
              {copiedKey === 'reference'
                ? 'Copied reference'
                : 'Copy reference'}
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={() => copyText('link', shareableUrl)}
              leftIcon={copiedKey === 'link' ? <Check /> : <Link2 />}
            >
              {copiedKey === 'link' ? 'Copied link' : 'Copy link'}
            </Button>
          </div>
        </CardHeader>
        <CardContent>
          <dl className="grid gap-3 md:grid-cols-2">
            <TheFact label="Repository" value={asset.repository} />
            <TheFact label="Path" value={asset.asset_path} mono />
            <TheFact label="Package" value={asset.package_name || '—'} />
            <TheFact label="Version" value={asset.package_version || '—'} />
            <TheFact label="Digest" value={digest} mono />
            <TheFact label="Size" value={theSize(asset.size_bytes)} />
            <TheFact label="Updated" value={theMoment(asset.updated_at)} />
            <TheFact label="Blob scope" value={asset.blob.scope} />
            {blobDigest !== digest && (
              <TheFact label="Blob digest" value={blobDigest} mono />
            )}
          </dl>
          <p className="mt-3 text-xs text-gray-500">
            The repository path is a mutable coordinate; the digest identifies
            the exact content bytes. Shareable links pin the expected digest and
            never include credentials.
          </p>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Publication</CardTitle>
        </CardHeader>
        <CardContent>
          <dl className="grid gap-3 md:grid-cols-2">
            <TheFact
              label="Published"
              value={theMoment(asset.published.published_at)}
            />
            {(asset.published.package_name !== asset.package_name ||
              asset.published.package_version !== asset.package_version) && (
              <TheFact
                label="Published package"
                value={`${asset.published.package_name} ${asset.published.package_version}`}
              />
            )}
            {publishedDigest !== digest && (
              <TheFact label="Published digest" value={publishedDigest} mono />
            )}
            {asset.published.size_bytes !== asset.size_bytes && (
              <TheFact
                label="Published size"
                value={theSize(asset.published.size_bytes)}
              />
            )}
          </dl>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Trust profile</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <p className="text-sm text-gray-600">
            The profile is the reading of the evidence, dimension by dimension.
            A dimension without evidence is not satisfied, and it is not read as
            a favourable one.
          </p>
          {asset.capsule.profile.dimensions.length === 0 ? (
            <p className="text-sm text-gray-500">
              There is no evidence about this content yet.
            </p>
          ) : (
            <ul className="flex flex-col divide-y divide-gray-100">
              {asset.capsule.profile.dimensions.map(dimension => (
                <li
                  key={dimension.name}
                  className="flex flex-wrap items-center justify-between gap-2 py-2"
                >
                  <span className="text-sm text-gray-800">
                    {dimension.name}
                  </span>
                  <span className="flex items-center gap-2">
                    <span className="text-xs text-gray-600">
                      {dimension.state}
                    </span>
                    {dimension.reason && (
                      <span className="text-xs text-gray-500">
                        {dimension.reason}
                      </span>
                    )}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Lineage</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <TheFact
            label="Subject"
            value={
              asset.lineage.subject
                ? `${asset.lineage.subject.algorithm}:${asset.lineage.subject.value}`
                : 'It does not hang from anything'
            }
            mono
          />
          {asset.lineage.referrers.length === 0 ? (
            <p className="text-sm text-gray-500">
              Nothing hangs from this manifest.
            </p>
          ) : (
            <ul className="flex flex-col divide-y divide-gray-100">
              {asset.lineage.referrers.map(referrer => (
                <li
                  key={referrer.asset_path}
                  className="flex flex-wrap items-center justify-between gap-2 py-2"
                >
                  <span className="text-sm text-gray-800">
                    {referrer.asset_path}
                  </span>
                  <span className="text-xs text-gray-500">
                    {theSize(referrer.size_bytes)}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </CardContent>
      </Card>

      <ConfirmDialog
        open={promoting}
        title="Promote to the trusted channel"
        description="The content will be served as trusted to everybody who reads the repository. This changes what the repository serves, and the person who promoted it cannot undo it from here."
        confirmLabel="Promote"
        pending={promote.isPending}
        onClose={() => setPromoting(false)}
        onConfirm={async () => {
          try {
            await promote.mutateAsync({
              repository: asset.repository,
              path: asset.asset_path,
              digest,
            });
            setPromoting(false);
          } catch {
            // The failure is shown by the dialog.
          }
        }}
      >
        <p>
          The digest <span className="font-mono">{digest}</span> will be granted
          as trusted.
        </p>
        {promote.error && <FailureNotice failure={failureOf(promote.error)} />}
        {theReasonsOf(promote.error).length > 0 && (
          <ul className="list-disc space-y-1 pl-5 text-sm text-red-700">
            {theReasonsOf(promote.error).map(reason => (
              <li key={reason}>{reason}</li>
            ))}
          </ul>
        )}
      </ConfirmDialog>

      <ConfirmDialog
        open={revoking}
        title="Revoke the concession"
        description="The content will stop being served as trusted. The repository keeps it, but nobody may read it as trusted anymore."
        confirmLabel="Revoke"
        destructive
        pending={revoke.isPending}
        disabled={reason.trim().length === 0}
        onClose={() => setRevoking(false)}
        onConfirm={async () => {
          try {
            await revoke.mutateAsync({
              repository: asset.repository,
              digest,
              reason: reason.trim(),
            });
            setRevoking(false);
            setReason('');
          } catch {
            // The failure is shown by the dialog.
          }
        }}
      >
        <div className="flex flex-col gap-2">
          <p>
            The concession over <span className="font-mono">{digest}</span> will
            be withdrawn.
          </p>
          <Input
            label="Reason"
            value={reason}
            onChange={event => setReason(event.target.value)}
            placeholder="The signature does not hold"
            helperText="The reason is kept with the revocation."
          />
        </div>
        {revoke.error && <FailureNotice failure={failureOf(revoke.error)} />}
      </ConfirmDialog>
    </div>
  );
};

interface TheFactProps {
  label: string;
  value: string;
  mono?: boolean;
}

const TheFact = ({ label, value, mono = false }: TheFactProps) => (
  <div>
    <dt className="text-xs uppercase tracking-wide text-gray-500">{label}</dt>
    <dd
      className={
        mono
          ? 'break-all font-mono text-sm text-gray-900'
          : 'break-words text-sm text-gray-900'
      }
    >
      {value}
    </dd>
  </div>
);
