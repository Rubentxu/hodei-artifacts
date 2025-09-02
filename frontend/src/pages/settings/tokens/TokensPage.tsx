import React, { useState } from 'react';
import { PageHeader } from '@/components/layout/PageHeader';
import { Button } from '@/components/ui/Button';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/Card';
import { DataTable } from '@/components/layout/DataTable';
import { Badge } from '@/components/ui/Badge';
import {
  Modal,
  ModalContent,
  ModalHeader,
  ModalTitle,
  ModalDescription,
  ModalFooter,
  ModalTrigger,
} from '@/components/ui/Modal';
import { Input } from '@/components/ui/Input';
import type { ApiToken } from '@/features/tokens';
import { useTokens } from '@/features/tokens';
import { useNotifications } from '@/shared/stores/ui.store';
import { useForm } from 'react-hook-form';
import type { Column } from '@/components/layout/DataTable';

const CreateTokenModal = ({ createToken, isCreating, onTokenCreated }) => {
  const {
    register,
    handleSubmit,
    formState: {},
  } = useForm({ defaultValues: { name: '', scopes: ['repo:read'] } });
  const [isOpen, setIsOpen] = useState(false);

  const onSubmit = async (data: NewApiToken) => {
    const result: CreatedApiToken = await createToken(data);
    if (result) {
      onTokenCreated(result.token);
      // Do not close modal, parent will handle it
    }
  };

  return (
    <Modal open={isOpen} onOpenChange={setIsOpen}>
      <ModalTrigger asChild>
        <Button>Generate New Token</Button>
      </ModalTrigger>
      <ModalContent>
        <ModalHeader>
          <ModalTitle>Generate New Token</ModalTitle>
          <ModalDescription>
            Create a new personal access token.
          </ModalDescription>
        </ModalHeader>
        <form onSubmit={handleSubmit(onSubmit)} className="space-y-4">
          <div>
            <label htmlFor="name">Token Name</label>
            <Input
              id="name"
              {...register('name', { required: 'Token name is required' })}
            />
            {errors.name && (
              <p className="text-sm text-red-500">{errors.name.message}</p>
            )}
          </div>
          {/* Add scope selection later */}
          <ModalFooter>
            <Button
              type="button"
              variant="ghost"
              onClick={() => setIsOpen(false)}
            >
              Cancel
            </Button>
            <Button type="submit" disabled={isCreating}>
              {isCreating ? 'Generating...' : 'Generate'}
            </Button>
          </ModalFooter>
        </form>
      </ModalContent>
    </Modal>
  );
};

const ShowTokenModal = ({
  token,
  onClose,
}: {
  token: string | null;
  onClose: () => void;
}) => {
  return (
    <Modal open={!!token} onOpenChange={onClose}>
      <ModalContent>
        <ModalHeader>
          <ModalTitle>New Token Generated</ModalTitle>
          <ModalDescription>
            Here is your new token. Copy it now, you won't be able to see it
            again.
          </ModalDescription>
        </ModalHeader>
        <div className="mt-4 p-3 bg-muted rounded-md font-mono text-sm break-all">
          {token}
        </div>
        <ModalFooter>
          <Button onClick={onClose}>Close</Button>
        </ModalFooter>
      </ModalContent>
    </Modal>
  );
};

const TokensPage = () => {
  const {
    tokens,
    createToken,
    revokeToken,
    isLoading,
    isCreating,
    isRevoking,
  } = useTokens();
  const [newToken, setNewToken] = useState<string | null>(null);

  const handleRevokeToken = (id: string) => {
    // TODO: Replace with a custom confirmation modal or a more robust notification system
    if (confirm('Are you sure you want to revoke this token?')) {
      revokeToken(id);
    }
  };

  const columns: Column<ApiToken>[] = [
    { key: 'name', title: 'Name' },
    {
      key: 'scopes',
      title: 'Scopes',
      render: scopes => (
        <div className="flex gap-1">
          {scopes.map(s => (
            <Badge key={s}>{s}</Badge>
          ))}
        </div>
      ),
    },
    { key: 'lastUsed', title: 'Last Used' },
    { key: 'created', title: 'Created' },
    {
      key: 'actions',
      title: 'Actions',
      render: (_, token: ApiToken) => (
        <div className="space-x-2">
          <Button
            variant="ghost"
            size="sm"
            onClick={() => handleRevokeToken(token.id)}
            disabled={isRevoking}
          >
            Revoke
          </Button>
        </div>
      ),
    },
  ];

  return (
    <div>
      <PageHeader
        title="API Tokens"
        subtitle="Manage your personal access tokens."
      >
        <CreateTokenModal
          createToken={createToken}
          isCreating={isCreating}
          onTokenCreated={setNewToken}
        />
      </PageHeader>
      <Card>
        <CardHeader>
          <CardTitle>Your Tokens</CardTitle>
        </CardHeader>
        <CardContent>
          <DataTable
            columns={columns}
            data={tokens || []}
            loading={isLoading}
          />
        </CardContent>
      </Card>
      <ShowTokenModal token={newToken} onClose={() => setNewToken(null)} />
    </div>
  );
};

export default TokensPage;
