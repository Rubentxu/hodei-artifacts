/**
 * The creation of a repository of the catalog.
 *
 * The form offers what the API says it can create: the formats come from
 * the capabilities endpoint, and the modalities of each format come with
 * it. A format that the release does not serve is not offered, and a
 * modality that the format does not serve is not offered either: the
 * console does not keep a table of its own, because a table of its own is
 * a second truth and a second truth drifts.
 *
 * If the capabilities endpoint cannot be loaded or reports no formats, the
 * dialog fails closed without offering fictitious fallback options.
 *
 * The fields of the settings are the ones the modality needs: the
 * upstream of a proxy, the members of a group.
 */

import { useMemo, useState } from 'react';

import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import {
  Modal,
  ModalContent,
  ModalDescription,
  ModalFooter,
  ModalHeader,
  ModalTitle,
} from '@/components/ui/Modal';
import { Spinner } from '@/components/ui/Spinner';

import { failureOf } from '../api/errors';
import { useCapabilities, useCreateRepository } from '../hooks/useConsole';
import { FailureNotice } from './FailureNotice';
import { formatLabel } from './formatLabels';

export interface RepositoryCreateDialogProps {
  open: boolean;
  onClose: () => void;
}

/** The label of a modality, as the console names it. */
const MODE_LABELS: Record<string, string> = {
  hosted: 'Hosted — what is published here is kept here',
  proxy: 'Proxy — what is asked for is brought from an upstream',
  group: 'Group — what is asked for is looked for in the members',
};

/**
 * The name of the upstream, out of the name of the repository.
 *
 * The upstream has its own rules for its name —lowercase letters, digits,
 * dashes and underscores, and sixty-four characters at most— and the
 * repository has others. The name that the upstream reads is made out of
 * the one of the repository, so that a repository called `MyLibs` can be
 * declared instead of refused with a rule about upstreams.
 */
const theUpstreamId = (name: string): string =>
  name
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, '-')
    .slice(0, 64);

export const RepositoryCreateDialog = ({
  open,
  onClose,
}: RepositoryCreateDialogProps) => {
  const capabilities = useCapabilities(open);
  const create = useCreateRepository();
  const [name, setName] = useState('');
  const [format, setFormat] = useState('');
  const [mode, setMode] = useState('');
  const [upstreamUrl, setUpstreamUrl] = useState('');
  const [upstreamScopes, setUpstreamScopes] = useState('');
  const [members, setMembers] = useState('');
  const [trusted, setTrusted] = useState(false);

  const formats = useMemo(
    () => capabilities.data?.formats ?? [],
    [capabilities.data]
  );
  const theFormat = useMemo(
    () => formats.find(candidate => candidate.name === format),
    [formats, format]
  );
  const modes = useMemo(() => theFormat?.modes ?? [], [theFormat]);
  const selectedMode = modes.includes(mode) ? mode : '';

  const capabilitiesReady =
    !capabilities.isLoading && !capabilities.error && formats.length > 0;
  const canSubmit =
    capabilitiesReady &&
    name.trim().length > 0 &&
    Boolean(theFormat) &&
    Boolean(selectedMode);

  const handleClose = () => {
    setName('');
    setFormat('');
    setMode('');
    setUpstreamUrl('');
    setUpstreamScopes('');
    setMembers('');
    setTrusted(false);
    create.reset();
    onClose();
  };

  const submit = async () => {
    if (!canSubmit || !theFormat) return;
    const settings: Record<string, unknown> = {};
    if (selectedMode === 'proxy') {
      settings.proxy = {
        upstream: {
          id: theUpstreamId(name),
          url: upstreamUrl.trim(),
          scopes: upstreamScopes
            .split(',')
            .map(scope => scope.trim())
            .filter(scope => scope.length > 0),
        },
      };
    }
    if (selectedMode === 'group') {
      settings.group = {
        members: members
          .split(',')
          .map(member => member.trim())
          .filter(member => member.length > 0),
      };
    }
    if (selectedMode === 'hosted' && trusted) {
      settings.trust = { serve: 'trusted' };
    }
    try {
      await create.mutateAsync({
        name: name.trim(),
        format: theFormat.name,
        mode: selectedMode,
        settings,
      });
      handleClose();
    } catch {
      // The failure is shown by the dialog: the kind of it is the thing
      // that has to be told apart.
    }
  };

  return (
    <Modal
      open={open}
      onOpenChange={(nextOpen: boolean) => !nextOpen && handleClose()}
    >
      <ModalContent className="max-w-xl">
        <ModalHeader>
          <ModalTitle>New repository</ModalTitle>
          <ModalDescription>
            The format and the modality are the ones the service serves.
          </ModalDescription>
        </ModalHeader>

        <div className="flex flex-col gap-4">
          {capabilities.isLoading ? (
            <div
              className="flex items-center gap-2 py-4 text-sm text-gray-600"
              role="status"
            >
              <Spinner />
              <span>Loading deployment capabilities…</span>
            </div>
          ) : capabilities.error ? (
            <div className="flex flex-col gap-2">
              <FailureNotice failure={failureOf(capabilities.error)} />
              <p className="text-sm text-gray-600">
                Repository creation is unavailable until deployment capabilities
                can be loaded.
              </p>
            </div>
          ) : formats.length === 0 ? (
            <div
              role="alert"
              className="rounded-md border border-amber-200 bg-amber-50 p-4 text-sm text-amber-800"
            >
              No repository formats are available in this deployment.
            </div>
          ) : (
            <>
              <Input
                label="Name"
                value={name}
                onChange={event => setName(event.target.value)}
                placeholder="releases"
              />

              <label className="flex flex-col gap-1 text-sm font-medium">
                Format
                <select
                  value={theFormat?.name ?? ''}
                  onChange={event => {
                    setFormat(event.target.value);
                    setMode('');
                  }}
                  className="rounded-md border border-gray-300 px-3 py-2 text-sm"
                >
                  <option value="">Choose a format</option>
                  {formats.map(candidate => (
                    <option key={candidate.name} value={candidate.name}>
                      {formatLabel(candidate.name)}
                    </option>
                  ))}
                </select>
              </label>

              {theFormat && (
                <fieldset className="flex flex-col gap-2">
                  <legend className="text-sm font-medium">Modality</legend>
                  {modes.length === 0 ? (
                    <p className="text-sm text-gray-500">
                      This format has no available modalities.
                    </p>
                  ) : (
                    modes.map(candidate => (
                      <label
                        key={candidate}
                        className="flex items-start gap-2 text-sm"
                      >
                        <input
                          type="radio"
                          name="mode"
                          value={candidate}
                          checked={selectedMode === candidate}
                          onChange={() => setMode(candidate)}
                          className="mt-1"
                        />
                        <span>{MODE_LABELS[candidate] ?? candidate}</span>
                      </label>
                    ))
                  )}
                </fieldset>
              )}

              {selectedMode === 'proxy' && (
                <>
                  <Input
                    label="Upstream"
                    value={upstreamUrl}
                    onChange={event => setUpstreamUrl(event.target.value)}
                    placeholder="https://registry.example.org/"
                    helperText={`The upstream is named "${theUpstreamId(name)}".`}
                  />
                  <Input
                    label="Scopes"
                    value={upstreamScopes}
                    onChange={event => setUpstreamScopes(event.target.value)}
                    placeholder="/, acme/"
                    helperText="The path prefixes that may be asked for upstream. One prefix per comma."
                  />
                </>
              )}

              {selectedMode === 'group' && (
                <Input
                  label="Members"
                  value={members}
                  onChange={event => setMembers(event.target.value)}
                  placeholder="libs, apps"
                  helperText="The repositories that are looked at, in order. One per comma."
                />
              )}

              {selectedMode === 'hosted' && (
                <label className="flex items-center gap-2 text-sm">
                  <input
                    type="checkbox"
                    checked={trusted}
                    onChange={event => setTrusted(event.target.checked)}
                  />
                  Serve only what has a live concession
                </label>
              )}

              {create.error && (
                <FailureNotice failure={failureOf(create.error)} />
              )}
            </>
          )}
        </div>

        <ModalFooter>
          <Button variant="outline" onClick={handleClose}>
            Cancel
          </Button>
          <Button
            onClick={submit}
            isLoading={create.isPending}
            disabled={!canSubmit}
          >
            Create
          </Button>
        </ModalFooter>
      </ModalContent>
    </Modal>
  );
};
