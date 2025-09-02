import { useAuth } from '@/shared/stores/auth.store';
import { Card } from '@/components/ui/Card';
import { Skeleton } from '@/components/ui/Skeleton';
import { useDashboardData } from '@/shared/hooks/repositories';

const MetricCardSkeleton = () => (
  <Card className="p-6">
    <div className="flex items-center justify-between mb-2">
      <Skeleton className="h-6 w-3/4" />
      <Skeleton className="h-8 w-8 rounded-full" />
    </div>
    <Skeleton className="h-8 w-1/2" />
  </Card>
);

const ListItemSkeleton = () => (
  <div className="flex items-center justify-between p-3">
    <div className="flex items-center space-x-3">
      <Skeleton className="h-8 w-8 rounded-full" />
      <div>
        <Skeleton className="h-4 w-32 mb-2" />
        <Skeleton className="h-3 w-24" />
      </div>
    </div>
    <Skeleton className="h-4 w-16" />
  </div>
);

const ActivityItemSkeleton = () => (
  <div className="flex items-center justify-between">
    <div className="flex items-center space-x-3">
      <Skeleton className="h-2 w-2 rounded-full" />
      <Skeleton className="h-4 w-48" />
    </div>
    <Skeleton className="h-4 w-12" />
  </div>
);

export const Dashboard = () => {
  const { user } = useAuth();
  const { data: dashboardData, isLoading: isLoadingDashboard } =
    useDashboardData(5);

  const metrics = dashboardData?.data?.metrics;
  const recentRepositories = dashboardData?.data?.recentRepositories || [];
  const recentActivity = dashboardData?.data?.recentActivity || [];

  return (
    <div className="p-6">
      <div className="flex items-center justify-between mb-8">
        <div>
          <h1 className="text-3xl font-bold text-gray-900">Dashboard</h1>
          <p className="text-gray-600 mt-1">
            Welcome back, {user?.name || user?.email}!
          </p>
        </div>
      </div>

      {/* Metrics Cards */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-6 mb-8">
        {isLoadingDashboard ? (
          <>
            <MetricCardSkeleton />
            <MetricCardSkeleton />
            <MetricCardSkeleton />
            <MetricCardSkeleton />
          </>
        ) : (
          <>
            <Card className="p-6">
              <h3 className="text-lg font-semibold text-gray-900">
                Total Packages
              </h3>
              <p className="text-3xl font-bold text-blue-600">
                {metrics?.totalPackages.toLocaleString() || '0'}
              </p>
            </Card>
            <Card className="p-6">
              <h3 className="text-lg font-semibold text-gray-900">
                Repositories
              </h3>
              <p className="text-3xl font-bold text-green-600">
                {metrics?.activeRepositories.toLocaleString() || '0'}
              </p>
            </Card>
            <Card className="p-6">
              <h3 className="text-lg font-semibold text-gray-900">
                Online Users
              </h3>
              <p className="text-3xl font-bold text-purple-600">
                {metrics?.onlineUsers.toLocaleString() || '0'}
              </p>
            </Card>
            <Card className="p-6">
              <h3 className="text-lg font-semibold text-gray-900">
                Storage Used
              </h3>
              <p className="text-3xl font-bold text-orange-600">
                {metrics?.storageUsed
                  ? `${metrics.storageUsed.value} ${metrics.storageUsed.unit}`
                  : '0 GB'}
              </p>
            </Card>
          </>
        )}
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        {/* Recent Repositories */}
        <Card className="p-6">
          <div className="flex items-center justify-between mb-4">
            <h3 className="text-xl font-semibold text-gray-900">
              Recent Repositories
            </h3>
          </div>
          <div className="space-y-3">
            {isLoadingDashboard ? (
              <>
                <ListItemSkeleton />
                <ListItemSkeleton />
                <ListItemSkeleton />
              </>
            ) : recentRepositories.length > 0 ? (
              recentRepositories.map(repo => (
                <div
                  key={repo.id}
                  className="flex items-center justify-between p-3 bg-gray-50 rounded-lg"
                >
                  <div className="flex items-center space-x-3">
                    <div className="w-8 h-8 bg-blue-100 rounded-full flex items-center justify-center">
                      <span className="text-sm font-medium text-blue-600">
                        {repo.type.charAt(0).toUpperCase()}
                      </span>
                    </div>
                    <div>
                      <h4 className="font-medium text-gray-900">{repo.name}</h4>
                      <p className="text-sm text-gray-500">
                        {repo.packageCount} packages •{' '}
                        {Math.round(repo.size / 1024 / 1024)} MB
                      </p>
                    </div>
                  </div>
                  <span className="text-sm text-gray-500">
                    {new Date(repo.lastUpdated).toLocaleDateString()}
                  </span>
                </div>
              ))
            ) : (
              <p className="text-gray-500 text-center py-4">
                No repositories found
              </p>
            )}
          </div>
        </Card>

        {/* Recent Activity */}
        <Card className="p-6">
          <h3 className="text-xl font-semibold text-gray-900 mb-4">
            Recent Activity
          </h3>
          <div className="space-y-3">
            {isLoadingDashboard ? (
              <>
                <ActivityItemSkeleton />
                <ActivityItemSkeleton />
                <ActivityItemSkeleton />
                <ActivityItemSkeleton />
                <ActivityItemSkeleton />
              </>
            ) : recentActivity.length > 0 ? (
              recentActivity.slice(0, 5).map(activity => (
                <div
                  key={activity.id}
                  className="flex items-center justify-between"
                >
                  <div className="flex items-center space-x-3">
                    <div
                      className={`w-2 h-2 rounded-full ${
                        activity.type === 'upload'
                          ? 'bg-green-400'
                          : activity.type === 'download'
                            ? 'bg-blue-400'
                            : activity.type === 'create'
                              ? 'bg-purple-400'
                              : 'bg-gray-400'
                      }`}
                    />
                    <span className="text-gray-600">
                      {activity.userName} {activity.type}d {activity.targetName}
                    </span>
                  </div>
                  <span className="text-sm text-gray-500">
                    {new Date(activity.timestamp).toLocaleTimeString()}
                  </span>
                </div>
              ))
            ) : (
              <p className="text-gray-500 text-center py-4">
                No recent activity
              </p>
            )}
          </div>
        </Card>
      </div>
    </div>
  );
};
