import React from 'react';
import { Card } from '@/components/ui/Card';
import { PageHeader } from '@/components/layout/PageHeader';

const ProfilePage = () => {
  return (
    <div>
      <PageHeader
        title="Profile"
        subtitle="Manage your account settings and preferences"
      />
      <Card className="p-6">
        <h3 className="text-lg font-semibold mb-4">Profile Settings</h3>
        <p className="text-gray-600">Profile management coming soon...</p>
      </Card>
    </div>
  );
};

export default ProfilePage;
