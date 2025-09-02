import React from 'react';
import { Skeleton } from '@/components/ui/Skeleton';
import { PageHeader } from '@/components/layout/PageHeader';
import { Card } from '@/components/ui/Card';

const SearchPageSkeleton: React.FC = () => {
  return (
    <div className="p-8">
      {/* Page Header Skeleton */}
      <PageHeader
        title={<Skeleton className="h-8 w-64" />}
        subtitle={<Skeleton className="h-5 w-96" />}
      />
      {/* Search Input and Save Button Skeleton */}
      <div className="flex items-center gap-2 mb-4">
        <Skeleton className="h-12 w-full" /> {/* Search Input */}
        <Skeleton className="h-12 w-32" /> {/* Save Search Button */}
      </div>
      {/* Main Content Area (Filters and Results) Skeleton */}
      <div className="flex">
        {/* Filters Sidebar Skeleton */}
        <aside className="w-1/4 p-4 border-r space-y-6">
          <Skeleton className="h-6 w-32 mb-4" /> {/* Filters Title */}
          {Array.from({ length: 3 }).map((_, i) => (
            <div key={i} className="space-y-2">
              <Skeleton className="h-5 w-24" /> {/* Facet Title */}
              <Skeleton className="h-4 w-full" />
              <Skeleton className="h-4 w-full" />
              <Skeleton className="h-4 w-full" />
            </div>
          ))}
          <Skeleton className="h-5 w-32 mt-6" /> {/* Favorite Searches Title */}
          <Skeleton className="h-4 w-full" />
          <Skeleton className="h-4 w-full" />
        </aside>
        {/* Search Results Skeleton */}
        <div className="flex-1 p-4 space-y-4">
          {/* Result Item Skeletons */}
          {Array.from({ length: 5 }).map((_, i) => (
            <Card key={i} className="p-4">
              <Skeleton className="h-6 w-3/4 mb-2" /> {/* Title */}
              <Skeleton className="h-4 w-full mb-1" />{' '}
              {/* Description line 1 */}
              <Skeleton className="h-4 w-2/3" /> {/* Description line 2 */}
              <div className="flex items-center gap-4 mt-2">
                <Skeleton className="h-5 w-20" /> {/* Badge */}
                <Skeleton className="h-4 w-24" /> {/* Version */}
                <Skeleton className="h-4 w-24" /> {/* Size */}
              </div>
            </Card>
          ))}
          <div className="flex justify-center mt-4">
            <Skeleton className="h-10 w-40" /> {/* Load More Button */}
          </div>
        </div>
      </div>
    </div>
  );
};

export default SearchPageSkeleton;
