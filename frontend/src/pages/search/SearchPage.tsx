import React from 'react';
import { PageHeader } from '@/components/layout/PageHeader';
import { Input } from '@/components/ui/Input';
import { Button } from '@/components/ui/Button';
import { Spinner } from '@/components/ui/Spinner';

const SearchPage = () => {
  return (
    <div>
      <PageHeader
        title="Search"
        subtitle="Search for artifacts and repositories across the system."
      />
      <div className="flex gap-2 mb-4">
        <Input placeholder="Search..." />
        <Button>Search</Button>
      </div>
      <div className="flex justify-center items-center p-8">
        <Spinner />
        <p className="ml-2">Search results will appear here.</p>
      </div>
    </div>
  );
};

export default SearchPage;
