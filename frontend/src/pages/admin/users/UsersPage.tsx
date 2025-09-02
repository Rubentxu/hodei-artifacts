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
  ModalTrigger,
} from '@/components/ui/Modal';
import type { User, NewUser, UpdateUser } from '@/features/users';
import { useUsers } from '@/features/users';
import { UserForm } from '@/features/users/components';
import type { Column } from '@/components/layout/DataTable';
import UsersPageSkeleton from '@/components/Users/UsersPageSkeleton';

const UserModal = ({
  user,
  onSubmit,
  isSubmitting,
  children,
}: {
  user?: User;
  onSubmit: (data: NewUser | UpdateUser, closeModal: () => void) => void;
  isSubmitting: boolean;
  children: React.ReactNode;
}) => {
  const [isOpen, setIsOpen] = useState(false);

  const handleSubmit = (data: NewUser | UpdateUser) => {
    onSubmit(data, () => setIsOpen(false));
  };

  return (
    <Modal open={isOpen} onOpenChange={setIsOpen}>
      <ModalTrigger asChild>{children}</ModalTrigger>
      <ModalContent>
        <ModalHeader>
          <ModalTitle>{user ? 'Edit User' : 'Add New User'}</ModalTitle>
        </ModalHeader>
        <UserForm
          user={user}
          onSubmit={handleSubmit}
          isSubmitting={isSubmitting}
        />
      </ModalContent>
    </Modal>
  );
};

const UsersPage = () => {
  const { users, isLoading, createUser, isCreating, updateUser, isUpdating } =
    useUsers();

  const handleCreateUser = (data: NewUser, closeModal: () => void) => {
    createUser(data, { onSuccess: closeModal });
  };

  const handleUpdateUser = (
    id: string,
    data: UpdateUser,
    closeModal: () => void
  ) => {
    updateUser({ id, data }, { onSuccess: closeModal });
  };

  const columns: Column<User>[] = [
    { key: 'name', title: 'Name' },
    { key: 'email', title: 'Email' },
    { key: 'role', title: 'Role', render: role => <Badge>{role}</Badge> },
    {
      key: 'status',
      title: 'Status',
      render: status => (
        <Badge variant={status === 'Active' ? 'success' : 'secondary'}>
          {status}
        </Badge>
      ),
    },
    {
      key: 'actions',
      title: 'Actions',
      render: (_, user: User) => (
        <div className="space-x-2">
          <UserModal
            user={user}
            onSubmit={(data, cb) => handleUpdateUser(user.id, data, cb)}
            isSubmitting={isUpdating}
          >
            <Button variant="ghost" size="sm">
              Edit
            </Button>
          </UserModal>
        </div>
      ),
    },
  ];

  return (
    <div>
      <PageHeader
        title="User Management"
        subtitle="Manage all users in the system."
      >
        <UserModal onSubmit={handleCreateUser} isSubmitting={isCreating}>
          <Button>Add User</Button>
        </UserModal>
      </PageHeader>
      <Card>
        <CardHeader>
          <CardTitle>All Users</CardTitle>
        </CardHeader>
        <CardContent>
          {isLoading ? (
            <UsersPageSkeleton />
          ) : (
            <DataTable
              columns={columns}
              data={users || []}
              loading={isLoading}
            />
          )}
        </CardContent>
      </Card>
    </div>
  );
};

export default UsersPage;
