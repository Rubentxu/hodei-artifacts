/**
 * The repositories of the catalog.
 *
 * The list is the one the API returns for the account of the verified
 * principal: the repositories that principal may read, and not one more.
 * A repository of another account is not in the list and its name is
 * never guessed, because saying that it exists is saying that it belongs
 * to somebody.
 *
 * The format, the modality and the channel are shown next to the name,
 * because they are what decides how the repository behaves and what it
 * serves.
 */

import { useState } from 'react';
import { Link } from 'react-router-dom';
import { Package, Plus, Server, Terminal } from 'lucide-react';

import { Badge } from '@/components/ui/Badge';
import { Button } from '@/components/ui/Button';
import { Card, CardContent } from '@/components/ui/Card';
import { Spinner } from '@/components/ui/Spinner';

import { failureOf } from '../api/errors';
import type {
  RepositoryMode,
  RepositorySummary,
  ServeChannel,
} from '../api/types';
import { useRepositories } from '../hooks/useConsole';
import { ClientConnectDialog } from './ClientConnectDialog';
import { FailureNotice } from './FailureNotice';
import { formatLabel } from './formatLabels';
import { RepositoryCreateDialog } from './RepositoryCreateDialog';

/** The label of a modality, as the console names it. */
const MODE_LABELS: Record<RepositoryMode, string> = {
  hosted: 'Hosted',
  proxy: 'Proxy',
  group: 'Group',
};

/** The label of a channel, as the console names it. */
const CHANNEL_LABELS: Record<ServeChannel, string> = {
  open: 'Open',
  trusted: 'Trusted',
};

/** The badge of the channel. */
const CHANNEL_VARIANT: Record<ServeChannel, 'info' | 'success'> = {
  open: 'info',
  trusted: 'success',
};

export const RepositoriesView = () => {
  const repositories = useRepositories();
  const [creating, setCreating] = useState(false);
  const [selectedForConnect, setSelectedForConnect] =
    useState<RepositorySummary | null>(null);

  if (repositories.isLoading) {
    return (
      <div className="flex items-center justify-center p-8" role="status">
        <Spinner />
        <span className="ml-2 text-sm text-gray-600">
          Loading the repositories…
        </span>
      </div>
    );
  }

  if (repositories.error) {
    return <FailureNotice failure={failureOf(repositories.error)} />;
  }

  const list = repositories.data?.repositories ?? [];

  return (
    <div className="flex flex-col gap-6">
      <div className="flex justify-end">
        <Button onClick={() => setCreating(true)} leftIcon={<Plus />}>
          New repository
        </Button>
      </div>

      {list.length === 0 ? (
        <Card>
          <CardContent className="flex flex-col items-center gap-2 py-10 text-center">
            <Server className="h-8 w-8 text-gray-400" aria-hidden="true" />
            <p className="text-sm font-medium text-gray-700">
              There is no repository you may read
            </p>
            <p className="text-sm text-gray-500">
              A repository that is not yours does not come here. Create one, or
              ask for the permission to read the ones that already exist.
            </p>
          </CardContent>
        </Card>
      ) : (
        <ul className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
          {list.map(repository => (
            <RepositoryCard
              key={repository.name}
              repository={repository}
              onConnect={() => setSelectedForConnect(repository)}
            />
          ))}
        </ul>
      )}

      <RepositoryCreateDialog
        open={creating}
        onClose={() => setCreating(false)}
      />
      <ClientConnectDialog
        repository={selectedForConnect}
        onClose={() => setSelectedForConnect(null)}
      />
    </div>
  );
};

interface RepositoryCardProps {
  repository: RepositorySummary;
  onConnect: () => void;
}

const RepositoryCard = ({ repository, onConnect }: RepositoryCardProps) => (
  <li>
    <Card className="h-full">
      <CardContent className="flex flex-col gap-3">
        <div className="flex items-start justify-between gap-2">
          <Link
            to={`/console/browse?repository=${encodeURIComponent(repository.name)}`}
            className="flex items-center gap-2 text-base font-semibold text-gray-900 hover:text-blue-700"
          >
            <Package className="h-4 w-4 text-gray-500" aria-hidden="true" />
            {repository.name}
          </Link>
          <Badge variant={CHANNEL_VARIANT[repository.channel]}>
            {CHANNEL_LABELS[repository.channel]}
          </Badge>
        </div>

        <dl className="flex flex-wrap gap-2 text-xs">
          <div className="rounded bg-gray-100 px-2 py-1">
            <dt className="inline text-gray-500">Format </dt>
            <dd className="inline font-medium text-gray-800">
              {formatLabel(repository.format)}
            </dd>
          </div>
          <div className="rounded bg-gray-100 px-2 py-1">
            <dt className="inline text-gray-500">Modality </dt>
            <dd className="inline font-medium text-gray-800">
              {MODE_LABELS[repository.mode] ?? repository.mode}
            </dd>
          </div>
        </dl>

        <div className="mt-auto pt-1">
          <Button
            type="button"
            variant="outline"
            size="sm"
            leftIcon={<Terminal className="h-3.5 w-3.5" />}
            onClick={onConnect}
            aria-label={`Connect client to ${repository.name}`}
          >
            Connect client
          </Button>
        </div>
      </CardContent>
    </Card>
  </li>
);
