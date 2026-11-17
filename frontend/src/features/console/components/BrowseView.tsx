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

import { useMemo, useState } from 'react';
import type { FormEvent } from 'react';
import { Link, useSearchParams } from 'react-router-dom';
import { Search } from 'lucide-react';

import { Badge } from '@/components/ui/Badge';
import { Button } from '@/components/ui/Button';
import { Card, CardContent } from '@/components/ui/Card';
import { Input } from '@/components/ui/Input';
import { Spinner } from '@/components/ui/Spinner';

import { failureOf } from '../api/errors';
import type { SearchHit, TrustState } from '../api/types';
import { useSearch } from '../hooks/useConsole';
import { FailureNotice } from './FailureNotice';

/** How many results the page brings. */
const PAGE_SIZE = 20;

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

export const BrowseView = () => {
  const [params, setParams] = useSearchParams();
  const [draft, setDraft] = useState({
    repository: params.get('repository') ?? '',
    package: params.get('package') ?? '',
    version: params.get('version') ?? '',
    text: params.get('text') ?? '',
  });

  const repository = params.get('repository') ?? '';
  const packageName = params.get('package') ?? '';
  const version = params.get('version') ?? '';
  const text = params.get('text') ?? '';
  const offset = Number(params.get('offset') ?? '0') || 0;

  const query = useMemo(
    () => ({
      repository: repository || undefined,
      package: packageName || undefined,
      version: version || undefined,
      text: text || undefined,
      limit: PAGE_SIZE,
      offset,
    }),
    [repository, packageName, version, text, offset]
  );

  const search = useSearch(query);

  const submit = (event: FormEvent) => {
    event.preventDefault();
    const next = new URLSearchParams();
    if (draft.repository) next.set('repository', draft.repository);
    if (draft.package) next.set('package', draft.package);
    if (draft.version) next.set('version', draft.version);
    if (draft.text) next.set('text', draft.text);
    setParams(next);
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

  return (
    <div className="flex flex-col gap-6">
      <form onSubmit={submit} className="flex flex-col gap-3" role="search">
        <div className="grid gap-3 md:grid-cols-4">
          <Input
            label="Repository"
            value={draft.repository}
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
        <div className="flex justify-end">
          <Button type="submit" leftIcon={<Search />}>
            Search
          </Button>
        </div>
      </form>

      {search.isFetching && (
        <div className="flex items-center gap-2 text-sm text-gray-600" role="status">
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
            />

            {search.data?.results.length === 0 ? (
              <p className="text-sm text-gray-600">
                Nothing matches what was asked for.
              </p>
            ) : (
              <ul className="flex flex-col divide-y divide-gray-100">
                {search.data?.results.map(hit => (
                  <TheResult key={`${hit.repository}/${hit.asset_path}`} hit={hit} />
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
}

/**
 * What the page says about how much there is.
 *
 * A counted total is said as it is. An uncounted one is said as "more",
 * because that is all that is known: the API did not walk the whole
 * catalog to count it, and the console does not invent a number.
 */
const TheSummary = ({ total, more, shown }: TheSummaryProps) => (
  <p className="text-sm text-gray-600" role="status">
    {total === null ? (
      more ? (
        <>More than {shown} results.</>
      ) : (
        <>{shown} results.</>
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
        {hit.repository} · {hit.asset_path}
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
