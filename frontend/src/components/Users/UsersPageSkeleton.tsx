import React from 'react';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/Card';
import { Skeleton } from '@/components/ui/Skeleton';
import { PageHeader } from '@/components/layout/PageHeader'; // Assuming this path is correct

const UsersPageSkeleton: React.FC = () => {
  return (
    <div>
      {/* Page Header Skeleton */}
      <PageHeader
        title={<Skeleton className="h-8 w-64" />}
        subtitle={<Skeleton className="h-5 w-96" />}
      >
        <Skeleton className="h-10 w-32" /> {/* Add User Button */}
      </PageHeader>

      {/* Card Skeleton */}
      <Card>
        <CardHeader>
          <CardTitle>
            <Skeleton className="h-6 w-40" /> {/* All Users Title */}
          </CardTitle>
        </CardHeader>
        <CardContent>
          {/* Table Header Skeleton */}
          <div className="grid grid-cols-5 gap-4 py-2 px-4 border-b border-gray-200">
            <Skeleton className="h-5 w-24" /> {/* Name */}
            <Skeleton className="h-5 w-32" /> {/* Email */}
            <Skeleton className="h-5 w-20" /> {/* Role */}
            <Skeleton className="h-5 w-24" /> {/* Status */}
            <Skeleton className="h-5 w-28" /> {/* Actions */}
          </div>
          {/* Table Rows Skeleton */}
          <div className="space-y-3 mt-2">
            {Array.from({ length: 5 }).map((_, i) => (
              <div key={i} className="grid grid-cols-5 gap-4 py-2 px-4">
                <Skeleton className="h-5 w-24" />
                <Skeleton className="h-5 w-32" />
                <Skeleton className="h-5 w-20" />
                <Skeleton className="h-5 w-24" />
                <Skeleton className="h-5 w-28" />
              </div>
            ))}
          </div>
        </CardContent>
      </Card>
    </div>
  );
};

export default UsersPageSkeleton;
