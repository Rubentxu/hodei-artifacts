import React, { useState } from 'react';
import { PageHeader } from '@/components/layout/PageHeader';
import { Button } from '@/components/ui/Button';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/Card';
import { Badge } from '@/components/ui/Badge';
import { DataTable } from '@/components/layout/DataTable';
import {
  Modal,
  ModalContent,
  ModalHeader,
  ModalTitle,
} from '@/components/ui/Modal';
import { Input } from '@/components/ui/Input';
import { CodeEditor } from '@/components/ui/CodeEditor';
import type { Policy, NewPolicy, UpdatePolicy } from '@/features/policies';
import { usePolicies } from '@/features/policies';
import { useForm, Controller } from 'react-hook-form';
import type { Column } from '@/components/layout/DataTable';

const PolicyForm = ({
  policy,
  onSubmit,
  isSubmitting,
}: {
  policy?: Policy;
  onSubmit: (data: NewPolicy | UpdatePolicy) => void;
  isSubmitting: boolean;
}) => {
  const {
    register,
    handleSubmit,
    control,
    formState: {},
  } = useForm<NewPolicy | UpdatePolicy>({
    defaultValues: policy || {
      name: '',
      description: '',
      effect: 'Permit',
      body: '',
    },
  });

  return (
    <form onSubmit={handleSubmit(onSubmit)} className="space-y-4">
      <Input
        {...register('name', { required: true })}
        placeholder="Policy Name"
      />
      <Input {...register('description')} placeholder="Policy Description" />
      <Controller
        name="body"
        control={control}
        render={({ field }) => (
          <CodeEditor value={field.value} onValueChange={field.onChange} />
        )}
      />
      <Button type="submit" disabled={isSubmitting}>
        {isSubmitting ? 'Saving...' : 'Save Policy'}
      </Button>
    </form>
  );
};

const PoliciesPage = () => {
  const {
    policies,
    isLoading,
    createPolicy,
    isCreating,
    updatePolicy,
    isUpdating,
    deletePolicy,
    isDeleting,
  } = usePolicies();
  const [isModalOpen, setIsModalOpen] = useState(false);
  const [selectedPolicy, setSelectedPolicy] = useState<Policy | undefined>(
    undefined
  );

  const handleFormSubmit = (data: NewPolicy | UpdatePolicy) => {
    if (selectedPolicy) {
      updatePolicy(
        { id: selectedPolicy.id, data },
        { onSuccess: () => setIsModalOpen(false) }
      );
    } else {
      createPolicy(data, { onSuccess: () => setIsModalOpen(false) });
    }
  };

  const openModal = (policy?: Policy) => {
    setSelectedPolicy(policy);
    setIsModalOpen(true);
  };

  const columns: Column<Policy>[] = [
    { key: 'name', title: 'Name' },
    { key: 'description', title: 'Description' },
    {
      key: 'effect',
      title: 'Effect',
      render: (effect: Policy['effect']) => (
        <Badge variant={effect === 'Permit' ? 'success' : 'danger'}>
          {effect}
        </Badge>
      ),
    },
    {
      key: 'actions',
      title: 'Actions',
      render: (_, policy: Policy) => (
        <div className="space-x-2">
          <Button variant="ghost" size="sm" onClick={() => openModal(policy)}>
            Edit
          </Button>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => deletePolicy(policy.id)}
            disabled={isDeleting}
          >
            Delete
          </Button>
        </div>
      ),
    },
  ];

  return (
    <div>
      <PageHeader
        title="Access Policies"
        subtitle="Manage ABAC policies for the system."
      >
        <Button onClick={() => openModal()}>Add Policy</Button>
      </PageHeader>
      <Card>
        <CardHeader>
          <CardTitle>All Policies</CardTitle>
        </CardHeader>
        <CardContent>
          <DataTable
            columns={columns}
            data={policies || []}
            loading={isLoading}
          />
        </CardContent>
      </Card>
      <Modal open={isModalOpen} onOpenChange={setIsModalOpen}>
        <ModalContent>
          <ModalHeader>
            <ModalTitle>
              {selectedPolicy ? 'Edit Policy' : 'Add New Policy'}
            </ModalTitle>
          </ModalHeader>
          <PolicyForm
            policy={selectedPolicy}
            onSubmit={handleFormSubmit}
            isSubmitting={isCreating || isUpdating}
          />
        </ModalContent>
      </Modal>
    </div>
  );
};

export default PoliciesPage;
