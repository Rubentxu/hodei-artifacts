/**
 * The browser of the assets of the catalog.
 *
 * The search asks the API for one page and the console shows that page.
 * When the API could not count how many matches there are, it says that
 * there are more and the console says it too: a total that was not
 * counted is not a total, and showing "0 results" when the page is full
 * is the kind of lie that makes somebody believe the catalog is empty.
 *
 * The filters are written the way the API reads them, and the page is a
 * fact of the request, not of the browser: the address of the console
 * carries what is being searched, so a link to a page of results is a
 * link somebody else can open.
 */

import { useEffect, useMemo, useState } from 'react';
import type { FormEvent } from 'react';
import { Link, useSearchParams } from 'react-router-dom';
import { Search } from 'lucide-react';

import { Badge } from '@/components/ui/Badge';
import { Button } from '@/components/ui/Button';
import { Card, CardContent } from '@/components/ui/Card';
import { Input } from '@/components/ui/Input';
import { Spinner } from '@/components/ui/Spinner';

import type { SearchParams } from '../api/consoleApi';
import { failureOf } from '../api/errors';
import type { RepositoryFormat, SearchHit, TrustState } from '../api/types';
import { useSearch } from '../hooks/useConsole';
import { FailureNotice } from './FailureNotice';

/** How many results the page brings. */
const PAGE_SIZE = 20;

/** Supported catalog formats for filtering via `attribute=format=<name>`. */
const FORMAT_OPTIONS: ReadonlyArray<{
  value: RepositoryFormat;
  label: string;
}> = [
  { value: 'raw', label: 'Raw' },
  { value: 'oci', label: 'OCI' },
  { value: 'maven2', label: 'Maven 2' },
  { value: 'npm', label: 'npm' },
  { value: 'pypi', label: 'PyPI' },
  { value: 'cargo', label: 'Cargo' },
  { value: 'nuget', label: 'NuGet' },
  { value: 'go', label: 'Go' },
  { value: 'helm', label: 'Helm' },
  { value: 'apt', label: 'APT' },
  { value: 'rpm', label: 'RPM' },
];

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

/** Parses a non-negative integer offset from URL search params. */
const parseOffset = (raw: string | null): number => {
  const parsed = Number(raw ?? '0');
  if (!Number.isFinite(parsed) || parsed <= 0) return 0;
  return Math.floor(parsed);
};

export interface BrowseViewProps {
  /**
   * When provided, scopes exploration to a single repository and locks
   * the repository filter to that name.
   */
  repository?: string;
}

export const BrowseView = ({
  repository: lockedRepository,
}: BrowseViewProps = {}) => {
  const [params, setParams] = useSearchParams();

  const urlRepository = params.get('repository') ?? '';
  const effectiveRepository = lockedRepository ?? urlRepository;
  const packageName = params.get('package') ?? '';
  const version = params.get('version') ?? '';
  const digest = params.get('digest') ?? '';
  const tag = params.get('tag') ?? '';
  const rawAttribute = params.get('attribute') ?? '';
  const format =
    params.get('format') ??
    (rawAttribute.startsWith('format=')
      ? rawAttribute.slice('format='.length)
      : '');
  const text =
    params.get('text') ?? params.get('q') ?? params.get('query') ?? '';
  const offset = parseOffset(params.get('offset'));

  const [draft, setDraft] = useState({
    repository: effectiveRepository,
    package: packageName,
    version,
    digest,
    tag,
    format,
    text,
  });

  // Keep form inputs synchronized with URL search params on back/forward navigation.
  useEffect(() => {
    setDraft({
      repository: effectiveRepository,
      package: packageName,
      version,
      digest,
      tag,
      format,
      text,
    });
  }, [effectiveRepository, packageName, version, digest, tag, format, text]);

  const attribute = format
    ? `format=${format}`
    : rawAttribute && !rawAttribute.startsWith('format=')
      ? rawAttribute
      : undefined;

  const query = useMemo<SearchParams>(
    () => ({
      repository: effectiveRepository || undefined,
      package: packageName || undefined,
      version: version || undefined,
      digest: digest || undefined,
      tag: tag || undefined,
      attribute,
      text: text || undefined,
      limit: PAGE_SIZE,
      offset,
    }),
    [
      effectiveRepository,
      packageName,
      version,
      digest,
      tag,
      attribute,
      text,
      offset,
    ]
  );

  const search = useSearch(query);

  const submit = (event: FormEvent) => {
    event.preventDefault();
    const next = new URLSearchParams();
    const repoValue = lockedRepository ? '' : draft.repository.trim();
    if (repoValue) next.set('repository', repoValue);
    if (draft.package.trim()) next.set('package', draft.package.trim());
    if (draft.version.trim()) next.set('version', draft.version.trim());
    if (draft.digest.trim()) next.set('digest', draft.digest.trim());
    if (draft.tag.trim()) next.set('tag', draft.tag.trim());
    if (draft.format) next.set('format', draft.format);
    else if (rawAttribute && !rawAttribute.startsWith('format=')) {
      next.set('attribute', rawAttribute);
    }
    if (draft.text.trim()) next.set('text', draft.text.trim());
    setParams(next);
  };

  const reset = () => {
    setDraft({
      repository: lockedRepository ?? '',
      package: '',
      version: '',
      digest: '',
      tag: '',
      format: '',
      text: '',
    });
    setParams(new URLSearchParams());
  };

  const goTo = (nextOffset: number) => {
    const next = new URLSearchParams(params);
    if (nextOffset <= 0) {
      next.delete('offset');
    } else {
      next.set('offset', String(nextOffset));
    }
    setParams(next);
  };

  const hasFilters = Boolean(
    (!lockedRepository && effectiveRepository) ||
      packageName ||
      version ||
      digest ||
      tag ||
      format ||
      text ||
      rawAttribute
  );

  return (
    <div className="flex flex-col gap-6">
      <form onSubmit={submit} className="flex flex-col gap-3" role="search">
        <div className="grid gap-3 md:grid-cols-4">
          <Input
            label="Repository"
            value={draft.repository}
            disabled={Boolean(lockedRepository)}
            onChange={event =>
              setDraft({ ...draft, repository: event.target.value })
            }
            placeholder="libs"
          />
          <Input
            label="Package"
            value={draft.package}
            onChange={event =>
              setDraft({ ...draft, package: event.target.value })
            }
            placeholder="requests"
          />
          <Input
            label="Version"
            value={draft.version}
            onChange={event =>
              setDraft({ ...draft, version: event.target.value })
            }
            placeholder="2.32.3"
          />
          <Input
            label="Text"
            value={draft.text}
            onChange={event => setDraft({ ...draft, text: event.target.value })}
            placeholder="anything in the asset"
          />
        </div>
        <div className="grid gap-3 md:grid-cols-3">
          <div className="w-full">
            <label
              htmlFor="browse-format"
              className="mb-1 block text-sm font-medium text-foreground"
            >
              Format
            </label>
            <select
              id="browse-format"
              value={draft.format}
              onChange={event =>
                setDraft({ ...draft, format: event.target.value })
              }
              className="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
            >
              <option value="">All formats</option>
              {FORMAT_OPTIONS.map(option => (
                <option key={option.value} value={option.value}>
                  {option.label}
                </option>
              ))}
            </select>
          </div>
          <Input
            label="Digest"
            value={draft.digest}
            onChange={event =>
              setDraft({ ...draft, digest: event.target.value })
            }
            placeholder="sha256:..."
          />
          <Input
            label="Tag"
            value={draft.tag}
            onChange={event => setDraft({ ...draft, tag: event.target.value })}
            placeholder="latest"
          />
        </div>
        <div className="flex justify-end gap-2">
          {hasFilters && (
            <Button type="button" variant="outline" onClick={reset}>
              Reset
            </Button>
          )}
          <Button type="submit" leftIcon={<Search />}>
            Search
          </Button>
        </div>
      </form>

      {search.isFetching && (
        <div
          className="flex items-center gap-2 text-sm text-gray-600"
          role="status"
        >
          <Spinner />
          Searching…
        </div>
      )}

      {search.error && <FailureNotice failure={failureOf(search.error)} />}

      {/* The page says nothing about how much there is until it has
          asked: a summary that says "0 results" while the search is on
          its way is a summary that says something that is not known. */}
      {!search.error && search.data && (
        <Card>
          <CardContent className="flex flex-col gap-3">
            <TheSummary
              total={search.data?.total ?? null}
              more={search.data?.more ?? false}
              shown={search.data?.results.length ?? 0}
              offset={offset}
            />

            {search.data?.results.length === 0 ? (
              <p className="text-sm text-gray-600">
                Nothing matches what was asked for.
              </p>
            ) : (
              <ul className="flex flex-col divide-y divide-gray-100">
                {search.data?.results.map(hit => (
                  <TheResult
                    key={`${hit.repository}/${hit.asset_path}`}
                    hit={hit}
                  />
                ))}
              </ul>
            )}

            <div className="flex items-center justify-between">
              <Button
                variant="outline"
                onClick={() => goTo(Math.max(0, offset - PAGE_SIZE))}
                disabled={offset === 0}
              >
                Previous
              </Button>
              <span className="text-xs text-gray-500">From {offset}</span>
              <Button
                variant="outline"
                onClick={() => goTo(offset + PAGE_SIZE)}
                disabled={!search.data?.more}
              >
                Next
              </Button>
            </div>
          </CardContent>
        </Card>
      )}
    </div>
  );
};

interface TheSummaryProps {
  total: number | null;
  more: boolean;
  shown: number;
  offset: number;
}

/**
 * What the page says about how much there is.
 *
 * A counted total is said as it is. An uncounted one is said as "more",
 * because that is all that is known: the API did not walk the whole
 * catalog to count it, and the console does not invent a number.
 */
const TheSummary = ({ total, more, shown, offset }: TheSummaryProps) => (
  <p className="text-sm text-gray-600" role="status">
    {total === null ? (
      more ? (
        shown > 0 ? (
          <>More than {offset + shown} results.</>
        ) : (
          <>More results available.</>
        )
      ) : (
        <>
          {shown} {shown === 1 ? 'result' : 'results'}.
        </>
      )
    ) : (
      <>
        {total} {total === 1 ? 'result' : 'results'}
        {more ? ', more after this page' : ''}
      </>
    )}
  </p>
);

interface TheResultProps {
  hit: SearchHit;
}

const TheResult = ({ hit }: TheResultProps) => (
  <li className="flex flex-wrap items-center justify-between gap-3 py-3">
    <div className="flex flex-col">
      <Link
        to={`/console/packages/${encodeURIComponent(hit.repository)}/${thePathOf(hit.asset_path)}`}
        className="text-sm font-medium text-gray-900 hover:text-blue-700"
      >
        {hit.package_name || hit.asset_path}
      </Link>
      <span className="text-xs text-gray-500">
        {hit.repository} · {hit.format} · {hit.asset_path}
      </span>
    </div>

    <div className="flex items-center gap-3">
      <span className="text-xs text-gray-600">{hit.package_version}</span>
      <span className="text-xs text-gray-500">{theSize(hit.size_bytes)}</span>
      <Badge variant={TRUST_VARIANT[hit.trust_state]}>
        {TRUST_LABELS[hit.trust_state]}
      </Badge>
    </div>
  </li>
);

/**
 * The path of an asset, with its slashes.
 *
 * The route is a wildcard and the path names the content inside the
 * repository, so the slashes are part of the name and each segment is
 * escaped on its own.
 */
const thePathOf = (path: string): string =>
  path
    .split('/')
    .filter(segment => segment.length > 0)
    .map(segment => encodeURIComponent(segment))
    .join('/');
