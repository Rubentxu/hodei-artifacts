/**
 * Contextual native client connection instructions for a repository.
 *
 * Displays the recipe matching the selected repository's format and modality,
 * using only a validated public base URL and never embedding secrets or
 * insecure client flags.
 */

import { useState } from 'react';
import { Check, Copy, Terminal } from 'lucide-react';

import { Badge } from '@/components/ui/Badge';
import { Button } from '@/components/ui/Button';
import {
  Modal,
  ModalContent,
  ModalDescription,
  ModalFooter,
  ModalHeader,
  ModalTitle,
} from '@/components/ui/Modal';

import type { RepositorySummary } from '../api/types';
import {
  buildClientRecipe,
  resolveValidatedBaseUrl,
} from '../clientConnect/recipes';
import { useCapabilities } from '../hooks/useConsole';

export interface ClientConnectDialogProps {
  repository: RepositorySummary | null;
  onClose: () => void;
}

export const ClientConnectDialog = ({
  repository,
  onClose,
}: ClientConnectDialogProps) => {
  const capabilities = useCapabilities();
  const [copiedStep, setCopiedStep] = useState<number | null>(null);

  const open = repository !== null;
  const resolvedBase = resolveValidatedBaseUrl(
    capabilities.data?.public_base_url
  );

  const recipe =
    repository && resolvedBase
      ? (() => {
          try {
            return buildClientRecipe(repository, resolvedBase.baseUrl);
          } catch {
            return null;
          }
        })()
      : null;

  const copyCommand = async (index: number, text: string) => {
    try {
      await navigator.clipboard?.writeText(text);
      setCopiedStep(index);
      setTimeout(() => {
        setCopiedStep(current => (current === index ? null : current));
      }, 2000);
    } catch {
      // Clipboard API may be unavailable in restricted contexts.
    }
  };

  return (
    <Modal open={open} onOpenChange={(next: boolean) => !next && onClose()}>
      <ModalContent className="max-w-2xl">
        <ModalHeader>
          <ModalTitle className="flex items-center gap-2">
            <Terminal className="h-5 w-5 text-gray-600" aria-hidden="true" />
            <span>
              Connect client{repository ? `: ${repository.name}` : ''}
            </span>
          </ModalTitle>
          <ModalDescription>
            {repository
              ? `Native client instructions for ${repository.format} (${repository.mode}).`
              : 'Select a repository to view connection instructions.'}
          </ModalDescription>
        </ModalHeader>

        {repository && !resolvedBase && (
          <div
            className="rounded-md border border-amber-200 bg-amber-50 p-3 text-sm text-amber-900"
            role="alert"
          >
            No validated public base URL is available. Configure{' '}
            <code>public_url.public_base_url</code> on the server or access the
            console over a valid HTTP/HTTPS origin.
          </div>
        )}

        {repository && resolvedBase && !recipe && (
          <div
            className="rounded-md border border-amber-200 bg-amber-50 p-3 text-sm text-amber-900"
            role="status"
          >
            No native client instructions are available for this repository format.
          </div>
        )}

        {repository && resolvedBase && recipe && (
          <div className="flex flex-col gap-4">
            <div className="flex flex-wrap items-center gap-2 text-xs">
              <Badge
                variant={
                  recipe.verificationStatus === 'VERIFIED'
                    ? 'success'
                    : 'warning'
                }
              >
                {recipe.verificationStatus}
              </Badge>
              <span className="rounded bg-gray-100 px-2 py-1 font-medium text-gray-800">
                Client: {recipe.clientLabel} {recipe.clientVersion}
              </span>
              <span className="rounded bg-gray-100 px-2 py-1 text-gray-700">
                Origin ({resolvedBase.source}):{' '}
                <code>{resolvedBase.baseUrl}</code>
              </span>
            </div>

            <div className="rounded-md border border-gray-200 bg-gray-50 p-3 text-xs text-gray-700">
              <div className="font-semibold text-gray-900">Endpoint</div>
              <code className="mt-1 block break-all text-gray-800">
                {recipe.endpointUrl}
              </code>
            </div>

            <div className="flex flex-col gap-3">
              {recipe.steps.map((step, index) => (
                <div
                  key={step.title}
                  className="rounded-md border border-gray-200 bg-white"
                >
                  <div className="flex items-center justify-between border-b border-gray-200 px-3 py-2">
                    <span className="text-xs font-semibold text-gray-800">
                      {step.title}
                    </span>
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      onClick={() => copyCommand(index, step.command)}
                      aria-label={`Copy ${step.title}`}
                    >
                      {copiedStep === index ? (
                        <Check className="h-3.5 w-3.5 text-green-600" />
                      ) : (
                        <Copy className="h-3.5 w-3.5" />
                      )}
                    </Button>
                  </div>
                  <pre className="overflow-x-auto p-3 text-xs text-gray-900">
                    <code>{step.command}</code>
                  </pre>
                </div>
              ))}
            </div>

            <p className="text-xs text-gray-600">{recipe.credentialGuidance}</p>
          </div>
        )}

        <ModalFooter>
          <Button variant="outline" onClick={onClose}>
            Close
          </Button>
        </ModalFooter>
      </ModalContent>
    </Modal>
  );
};
