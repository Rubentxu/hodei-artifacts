import React from 'react';
import { Card } from '@/components/ui/Card';
import { Skeleton } from '@/components/ui/Skeleton';

const RepositoryDetailSkeleton: React.FC = () => {
  return (
    <div className="p-6">
      {/* Breadcrumb Skeleton */}
      <div className="mb-6">
        <Skeleton className="h-4 w-64" />
      </div>

      {/* Repository Header Skeleton */}
      <Card className="p-6 mb-6">
        <div className="flex items-start justify-between">
          <div className="flex items-center gap-4">
            <Skeleton className="h-12 w-12 rounded-full" /> {/* Icon */}
            <div>
              <div className="flex items-center gap-3 mb-2">
                <Skeleton className="h-8 w-64" /> {/* Repository Name */}
                <Skeleton className="h-6 w-24 rounded-full" />{' '}
                {/* Type Badge */}
                <Skeleton className="h-6 w-24 rounded-full" />{' '}
                {/* Visibility Badge */}
              </div>
              <Skeleton className="h-5 w-96 mb-3" /> {/* Description */}
              <div className="flex items-center gap-6 text-sm text-gray-500">
                <Skeleton className="h-4 w-40" /> {/* URL */}
                <Skeleton className="h-4 w-32" /> {/* Packages */}
                <Skeleton className="h-4 w-32" /> {/* Size */}
              </div>
            </div>
          </div>
          <div className="flex items-center gap-2">
            <Skeleton className="h-10 w-24" /> {/* Settings Button */}
            <Skeleton className="h-10 w-28" /> {/* Permissions Button */}
            <Skeleton className="h-10 w-28" /> {/* Copy URL Button */}
          </div>
        </div>
      </Card>

      {/* Tabs Skeleton */}
      <div className="mb-6">
        <div className="border-b border-gray-200">
          <nav className="-mb-px flex space-x-8">
            <Skeleton className="h-8 w-24" />
            <Skeleton className="h-8 w-24" />
            <Skeleton className="h-8 w-24" />
            <Skeleton className="h-8 w-24" />
          </nav>
        </div>
      </div>

      {/* Tab Content Skeleton */}
      <Card className="p-6">
        <Skeleton className="h-6 w-48 mb-4" /> {/* Section Title */}
        <div className="space-y-4">
          <Skeleton className="h-10 w-full" />
          <Skeleton className="h-10 w-full" />
          <Skeleton className="h-10 w-full" />
          <Skeleton className="h-10 w-full" />
        </div>
      </Card>
    </div>
  );
};

export default RepositoryDetailSkeleton;
