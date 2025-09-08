import React from 'react';
import { Card } from '@/components/ui/Card';
import { PageHeader } from '@/components/layout/PageHeader';

const TokensPage = () => {
  return (
    <div>
      <PageHeader
        title="API Tokens"
        subtitle="Manage your API tokens for programmatic access"
      />
      <Card className="p-6">
        <h3 className="text-lg font-semibold mb-4">API Tokens</h3>
        <p className="text-gray-600">API token management coming soon...</p>
      </Card>
    </div>
  );
};

export default TokensPage;
