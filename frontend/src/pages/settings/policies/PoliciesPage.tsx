import React from 'react';
import { Card } from '@/components/ui/Card';
import { PageHeader } from '@/components/layout/PageHeader';

const PoliciesPage = () => {
  return (
    <div>
      <PageHeader
        title="Access Policies"
        subtitle="Manage Cedar policies for access control"
      />
      <Card className="p-6">
        <h3 className="text-lg font-semibold mb-4">Access Policies</h3>
        <p className="text-gray-600">Policy management coming soon...</p>
      </Card>
    </div>
  );
};

export default PoliciesPage;
