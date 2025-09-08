import React from 'react';
import { Card } from '@/components/ui/Card';
import { PageHeader } from '@/components/layout/PageHeader';

const UsersPage = () => {
  return (
    <div>
      <PageHeader
        title="User Management"
        subtitle="Manage system users and their permissions"
      />
      <Card className="p-6">
        <h3 className="text-lg font-semibold mb-4">Users</h3>
        <p className="text-gray-600">User management coming soon...</p>
      </Card>
    </div>
  );
};

export default UsersPage;
