import React, { useState, useEffect } from 'react';
import { Card } from '@/components/ui/Card';
import { PageHeader } from '@/components/layout/PageHeader';
import { Button } from '@/components/ui/Button';
import { Badge } from '@/components/ui/Badge';
import { mockAdapter } from '@/shared/api/mockAdapter';
import { repositoryServiceMock } from '@/shared/api/mockAdapter';
import { searchServiceMock } from '@/shared/api/mockAdapter';
import type { Repository, PackageResult } from '@/shared/types/openapi.types';
import {
  TrendingUp,
  Package,
  Users,
  HardDrive,
  Download,
  Clock,
  Activity,
} from 'lucide-react';

const DashboardEnhanced = () => {
  const [repositories, setRepositories] = useState<Repository[]>([]);
  const [popularPackages, setPopularPackages] = useState<PackageResult[]>([]);
  const [recentPackages, setRecentPackages] = useState<PackageResult[]>([]);
  const [totalDownloads, setTotalDownloads] = useState(0);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    loadDashboardData();
  }, []);

  const loadDashboardData = async () => {
    try {
      setLoading(true);
      setError(null);

      // Cargar datos en paralelo
      const [reposData, popularData, recentData] = await Promise.all([
        repositoryServiceMock.getRepositories(),
        searchServiceMock.getPopularPackages(5),
        searchServiceMock.getRecentPackages(5),
      ]);

      setRepositories(reposData.items);
      setPopularPackages(popularData);
      setRecentPackages(recentData);

      // Calcular total de descargas
      const total = [...popularData, ...recentData].reduce(
        (sum, pkg) => sum + pkg.downloadCount,
        0
      );
      setTotalDownloads(total);
    } catch (error) {
      console.error('Error loading dashboard data:', error);
      setError('Failed to load dashboard data. Please try again later.');
    } finally {
      setLoading(false);
    }
  };

  const getPackageTypeIcon = (type: string) => {
    switch (type) {
      case 'npm':
        return <Package className="w-4 h-4 text-green-500" />;
      case 'maven':
        return <Package className="w-4 h-4 text-blue-500" />;
      case 'pypi':
        return <Package className="w-4 h-4 text-yellow-500" />;
      default:
        return <Package className="w-4 h-4 text-gray-500" />;
    }
  };

  const getPackageTypeColor = (type: string) => {
    switch (type) {
      case 'npm':
        return 'bg-green-100 text-green-800';
      case 'maven':
        return 'bg-blue-100 text-blue-800';
      case 'pypi':
        return 'bg-yellow-100 text-yellow-800';
      default:
        return 'bg-gray-100 text-gray-800';
    }
  };

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64">
        <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-primary-600"></div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="flex flex-col items-center justify-center h-64 text-center">
        <div className="text-red-500 mb-4">
          <svg
            className="w-12 h-12 mx-auto"
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth={2}
              d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
            />
          </svg>
        </div>
        <h3 className="text-lg font-semibold text-gray-900 mb-2">
          Error Loading Dashboard
        </h3>
        <p className="text-gray-600 mb-4">{error}</p>
        <Button onClick={loadDashboardData} variant="outline">
          Try Again
        </Button>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <PageHeader
        title="Dashboard"
        subtitle="Overview of your artifact repositories"
        actions={
          <div className="flex gap-2">
            <Button variant="outline" size="sm" onClick={loadDashboardData}>
              <TrendingUp className="w-4 h-4 mr-2" />
              Refresh Data
            </Button>
            <Button size="sm">
              <Package className="w-4 h-4 mr-2" />
              Upload Artifact
            </Button>
          </div>
        }
      />

      {/* Stats Grid - Inspirado en JFrog Artifactory */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-6">
        <Card className="p-6 hover:shadow-lg transition-shadow">
          <div className="flex items-center justify-between">
            <div>
              <p className="text-sm font-medium text-gray-600">
                Total Repositories
              </p>
              <p className="text-3xl font-bold text-gray-900">
                {repositories.length}
              </p>
            </div>
            <div className="p-3 bg-blue-100 rounded-full">
              <HardDrive className="w-6 h-6 text-blue-600" />
            </div>
          </div>
          <div className="mt-4 flex items-center text-sm">
            <TrendingUp className="w-4 h-4 text-green-500 mr-1" />
            <span className="text-green-500">+12%</span>
            <span className="text-gray-600 ml-1">from last month</span>
          </div>
        </Card>

        <Card className="p-6 hover:shadow-lg transition-shadow">
          <div className="flex items-center justify-between">
            <div>
              <p className="text-sm font-medium text-gray-600">
                Total Downloads
              </p>
              <p className="text-3xl font-bold text-gray-900">
                {totalDownloads.toLocaleString()}
              </p>
            </div>
            <div className="p-3 bg-green-100 rounded-full">
              <Download className="w-6 h-6 text-green-600" />
            </div>
          </div>
          <div className="mt-4 flex items-center text-sm">
            <TrendingUp className="w-4 h-4 text-green-500 mr-1" />
            <span className="text-green-500">+23%</span>
            <span className="text-gray-600 ml-1">from last month</span>
          </div>
        </Card>

        <Card className="p-6 hover:shadow-lg transition-shadow">
          <div className="flex items-center justify-between">
            <div>
              <p className="text-sm font-medium text-gray-600">
                Popular Packages
              </p>
              <p className="text-3xl font-bold text-gray-900">
                {popularPackages.length}
              </p>
            </div>
            <div className="p-3 bg-purple-100 rounded-full">
              <Package className="w-6 h-6 text-purple-600" />
            </div>
          </div>
          <div className="mt-4 flex items-center text-sm">
            <Activity className="w-4 h-4 text-blue-500 mr-1" />
            <span className="text-blue-500">Most downloaded</span>
          </div>
        </Card>

        <Card className="p-6 hover:shadow-lg transition-shadow">
          <div className="flex items-center justify-between">
            <div>
              <p className="text-sm font-medium text-gray-600">
                Recent Activity
              </p>
              <p className="text-3xl font-bold text-gray-900">
                {recentPackages.length}
              </p>
            </div>
            <div className="p-3 bg-orange-100 rounded-full">
              <Clock className="w-6 h-6 text-orange-600" />
            </div>
          </div>
          <div className="mt-4 flex items-center text-sm">
            <Activity className="w-4 h-4 text-orange-500 mr-1" />
            <span className="text-orange-500">Last 24 hours</span>
          </div>
        </Card>
      </div>

      {/* Main Content Grid */}
      <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        {/* Repositories Section */}
        <div className="lg:col-span-2 space-y-6">
          <Card>
            <div className="p-6 border-b border-gray-200">
              <div className="flex items-center justify-between">
                <h3 className="text-lg font-semibold text-gray-900">
                  Repositories
                </h3>
                <Button variant="outline" size="sm">
                  View All
                </Button>
              </div>
            </div>
            <div className="p-6">
              <div className="space-y-4">
                {repositories.map(repo => (
                  <div
                    key={repo.id}
                    className="flex items-center justify-between p-4 bg-gray-50 rounded-lg hover:bg-gray-100 transition-colors cursor-pointer"
                  >
                    <div className="flex items-center space-x-3">
                      <div className="p-2 bg-blue-100 rounded-lg">
                        <HardDrive className="w-5 h-5 text-blue-600" />
                      </div>
                      <div>
                        <h4 className="font-medium text-gray-900">
                          {repo.name}
                        </h4>
                        <p className="text-sm text-gray-600">
                          {repo.description}
                        </p>
                      </div>
                    </div>
                    <div className="flex items-center space-x-2">
                      <Badge variant="secondary">
                        {repo.createdAt.split('T')[0]}
                      </Badge>
                      <Button variant="ghost" size="sm">
                        →
                      </Button>
                    </div>
                  </div>
                ))}
              </div>
            </div>
          </Card>

          {/* Activity Feed - Inspirado en GitHub */}
          <Card>
            <div className="p-6 border-b border-gray-200">
              <h3 className="text-lg font-semibold text-gray-900">
                Recent Activity
              </h3>
            </div>
            <div className="p-6">
              <div className="space-y-4">
                <div className="flex items-start space-x-3">
                  <div className="w-2 h-2 bg-green-500 rounded-full mt-2"></div>
                  <div className="flex-1">
                    <p className="text-sm text-gray-900">
                      <span className="font-medium">react</span> package
                      uploaded to npm-public
                    </p>
                    <p className="text-xs text-gray-500">2 hours ago</p>
                  </div>
                </div>
                <div className="flex items-start space-x-3">
                  <div className="w-2 h-2 bg-blue-500 rounded-full mt-2"></div>
                  <div className="flex-1">
                    <p className="text-sm text-gray-900">
                      New repository{' '}
                      <span className="font-medium">spring-boot-configs</span>{' '}
                      created
                    </p>
                    <p className="text-xs text-gray-500">5 hours ago</p>
                  </div>
                </div>
                <div className="flex items-start space-x-3">
                  <div className="w-2 h-2 bg-purple-500 rounded-full mt-2"></div>
                  <div className="flex-1">
                    <p className="text-sm text-gray-900">
                      <span className="font-medium">junit</span> downloaded 234
                      times
                    </p>
                    <p className="text-xs text-gray-500">1 day ago</p>
                  </div>
                </div>
              </div>
            </div>
          </Card>
        </div>

        {/* Sidebar - Popular & Recent Packages */}
        <div className="space-y-6">
          {/* Popular Packages */}
          <Card>
            <div className="p-6 border-b border-gray-200">
              <h3 className="text-lg font-semibold text-gray-900">
                Popular Packages
              </h3>
            </div>
            <div className="p-6">
              <div className="space-y-4">
                {popularPackages.map(pkg => (
                  <div
                    key={pkg.id}
                    className="flex items-center justify-between"
                  >
                    <div className="flex items-center space-x-3">
                      {getPackageTypeIcon(pkg.packageType)}
                      <div>
                        <h4 className="font-medium text-gray-900">
                          {pkg.name}
                        </h4>
                        <p className="text-xs text-gray-600">v{pkg.version}</p>
                      </div>
                    </div>
                    <div className="text-right">
                      <Badge variant="secondary">
                        {pkg.downloadCount.toLocaleString()}
                      </Badge>
                    </div>
                  </div>
                ))}
              </div>
            </div>
          </Card>

          {/* Recent Packages */}
          <Card>
            <div className="p-6 border-b border-gray-200">
              <h3 className="text-lg font-semibold text-gray-900">
                Recent Packages
              </h3>
            </div>
            <div className="p-6">
              <div className="space-y-4">
                {recentPackages.map(pkg => (
                  <div
                    key={pkg.id}
                    className="flex items-center justify-between"
                  >
                    <div className="flex items-center space-x-3">
                      {getPackageTypeIcon(pkg.packageType)}
                      <div>
                        <h4 className="font-medium text-gray-900">
                          {pkg.name}
                        </h4>
                        <p className="text-xs text-gray-600">
                          {pkg.repositoryName}
                        </p>
                      </div>
                    </div>
                    <Badge className={getPackageTypeColor(pkg.packageType)}>
                      {pkg.packageType}
                    </Badge>
                  </div>
                ))}
              </div>
            </div>
          </Card>
        </div>
      </div>

      {/* Quick Actions */}
      <Card>
        <div className="p-6">
          <h3 className="text-lg font-semibold text-gray-900 mb-4">
            Quick Actions
          </h3>
          <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
            <Button variant="outline" className="justify-start">
              <Package className="w-4 h-4 mr-2" />
              Upload Artifact
            </Button>
            <Button variant="outline" className="justify-start">
              <HardDrive className="w-4 h-4 mr-2" />
              Create Repository
            </Button>
            <Button variant="outline" className="justify-start">
              <Users className="w-4 h-4 mr-2" />
              Manage Users
            </Button>
          </div>
        </div>
      </Card>
    </div>
  );
};

export default DashboardEnhanced;
