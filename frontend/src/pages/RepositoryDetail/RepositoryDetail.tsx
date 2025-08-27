import { useState } from 'react';
import { notificationService } from '@/shared/stores/notification.store';
import { useParams, Navigate } from 'react-router-dom';
import { Card } from '@/components/ui/Card';
import { Button } from '@/components/ui/Button';
import { Spinner } from '@/components/ui/Spinner';
import { useRepository } from '@/shared/hooks/repositories';
import type { RepositoryType } from '@/shared/types';

interface Tab {
  id: string;
  label: string;
  icon: string;
}

const tabs: Tab[] = [
  { id: 'artifacts', label: 'Artifacts', icon: '📦' },
  { id: 'settings', label: 'Settings', icon: '⚙️' },
  { id: 'permissions', label: 'Permissions', icon: '🔐' },
  { id: 'activity', label: 'Activity', icon: '📊' },
];

export const RepositoryDetail = () => {
  const { id } = useParams<{ id: string }>();
  const [activeTab, setActiveTab] = useState('artifacts');

  const { data: repository, isLoading, error } = useRepository(id!);

  if (!id) {
    return <Navigate to="/repositories" replace />;
  }

  if (isLoading) {
    return (
      <div className="min-h-screen flex items-center justify-center">
        <Spinner size="lg" />
      </div>
    );
  }

  if (error) {
    return (
      <div className="p-6">
        <div className="bg-red-50 border border-red-200 rounded-lg p-4">
          <h3 className="text-lg font-semibold text-red-800 mb-2">
            Error loading repository
          </h3>
          <p className="text-red-600">
            Please try refreshing the page or contact support.
          </p>
        </div>
      </div>
    );
  }

  if (!repository) {
    return (
      <div className="p-6">
        <div className="bg-yellow-50 border border-yellow-200 rounded-lg p-4">
          <h3 className="text-lg font-semibold text-yellow-800 mb-2">
            Repository not found
          </h3>
          <p className="text-yellow-600">
            The repository you're looking for doesn't exist or you don't have
            access to it.
          </p>
        </div>
      </div>
    );
  }

  const getRepositoryIcon = (type: RepositoryType): string => {
    switch (type) {
      case 'maven':
        return '☕';
      case 'npm':
        return '📦';
      case 'pypi':
        return '🐍';
      case 'docker':
        return '🐳';
      default:
        return '📁';
    }
  };

  const getRepositoryTypeColor = (type: RepositoryType): string => {
    switch (type) {
      case 'maven':
        return 'bg-orange-100 text-orange-800';
      case 'npm':
        return 'bg-red-100 text-red-800';
      case 'pypi':
        return 'bg-blue-100 text-blue-800';
      case 'docker':
        return 'bg-cyan-100 text-cyan-800';
      default:
        return 'bg-gray-100 text-gray-800';
    }
  };

  const getVisibilityBadge = (isPublic: boolean): string => {
    return isPublic
      ? 'bg-green-100 text-green-800'
      : 'bg-yellow-100 text-yellow-800';
  };

  const formatSize = (bytes: number): string => {
    const units = ['B', 'KB', 'MB', 'GB', 'TB'];
    let size = bytes;
    let unitIndex = 0;

    while (size >= 1024 && unitIndex < units.length - 1) {
      size /= 1024;
      unitIndex++;
    }

    return `${Math.round(size * 100) / 100} ${units[unitIndex]}`;
  };

  const copyToClipboard = async (text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      notificationService.success('Copied!', 'Text copied to clipboard', 2000);
    } catch (error) {
      console.warn('Failed to copy to clipboard:', error);
      notificationService.error('Copy Failed', 'Failed to copy to clipboard');
    }
  };

  const renderTabContent = () => {
    switch (activeTab) {
      case 'artifacts':
        return (
          <div className="space-y-4">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <span className="text-lg">🔍</span>
                <input
                  type="text"
                  placeholder="Search artifacts..."
                  className="px-3 py-2 border border-gray-300 rounded-md shadow-sm focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-blue-500 w-64"
                />
              </div>
              <Button>
                <span className="mr-2">⬆️</span>
                Upload Artifact
              </Button>
            </div>

            <Card className="p-4">
              <div className="text-center py-12 text-gray-500">
                <div className="text-6xl mb-4">📁</div>
                <h3 className="text-lg font-medium mb-2">No artifacts found</h3>
                <p>Upload your first artifact to get started.</p>
              </div>
            </Card>
          </div>
        );

      case 'settings':
        return (
          <Card className="p-6">
            <h3 className="text-lg font-semibold mb-4">Repository Settings</h3>
            <div className="space-y-4">
              <div>
                <label className="block text-sm font-medium text-gray-700 mb-1">
                  Repository Name
                </label>
                <input
                  type="text"
                  value={repository.name}
                  className="w-full px-3 py-2 border border-gray-300 rounded-md"
                  readOnly
                />
              </div>
              <div>
                <label className="block text-sm font-medium text-gray-700 mb-1">
                  Description
                </label>
                <textarea
                  value={repository.description}
                  className="w-full px-3 py-2 border border-gray-300 rounded-md"
                  rows={3}
                  readOnly
                />
              </div>
              <div>
                <label className="block text-sm font-medium text-gray-700 mb-1">
                  Repository URL
                </label>
                <div className="flex gap-2">
                  <input
                    type="text"
                    value={repository.url}
                    className="flex-1 px-3 py-2 border border-gray-300 rounded-md"
                    readOnly
                  />
                  <Button
                    variant="outline"
                    onClick={() => copyToClipboard(repository.url)}
                  >
                    📋 Copy
                  </Button>
                </div>
              </div>
            </div>
          </Card>
        );

      case 'permissions':
        return (
          <Card className="p-6">
            <h3 className="text-lg font-semibold mb-4">Access Control</h3>
            <div className="text-center py-8 text-gray-500">
              <div className="text-4xl mb-2">🔐</div>
              <p>Permission management coming soon</p>
            </div>
          </Card>
        );

      case 'activity':
        return (
          <Card className="p-6">
            <h3 className="text-lg font-semibold mb-4">Repository Activity</h3>
            <div className="text-center py-8 text-gray-500">
              <div className="text-4xl mb-2">📊</div>
              <p>Activity feed coming soon</p>
            </div>
          </Card>
        );

      default:
        return null;
    }
  };

  return (
    <div className="p-6">
      {/* Breadcrumb */}
      <nav className="text-sm breadcrumbs mb-6">
        <div className="flex items-center space-x-2 text-gray-500">
          <a href="/" className="hover:text-gray-700">
            Home
          </a>
          <span>›</span>
          <a href="/repositories" className="hover:text-gray-700">
            Repositories
          </a>
          <span>›</span>
          <span className="text-gray-900 font-medium">{repository.name}</span>
        </div>
      </nav>

      {/* Repository Header */}
      <Card className="p-6 mb-6">
        <div className="flex items-start justify-between">
          <div className="flex items-center gap-4">
            <span className="text-4xl">
              {getRepositoryIcon(repository.type)}
            </span>
            <div>
              <div className="flex items-center gap-3 mb-2">
                <h1 className="text-3xl font-bold text-gray-900">
                  {repository.name}
                </h1>
                <span
                  className={`inline-flex items-center px-3 py-1 rounded-full text-sm font-medium ${getRepositoryTypeColor(repository.type)}`}
                >
                  {repository.type.toUpperCase()}
                </span>
                <span
                  className={`inline-flex items-center px-3 py-1 rounded-full text-sm font-medium ${getVisibilityBadge(repository.isPublic)}`}
                >
                  {repository.isPublic ? 'Public' : 'Private'}
                </span>
              </div>
              <p className="text-gray-600 text-lg mb-3">
                {repository.description}
              </p>
              <div className="flex items-center gap-6 text-sm text-gray-500">
                <span>🔗 {repository.url}</span>
                <span>
                  📊 {repository.packageCount.toLocaleString()} packages
                </span>
                <span>💾 {formatSize(repository.size)}</span>
              </div>
            </div>
          </div>
          <div className="flex items-center gap-2">
            <Button variant="outline">🔧 Settings</Button>
            <Button variant="outline">🔐 Permissions</Button>
            <Button
              variant="outline"
              onClick={() => copyToClipboard(repository.url)}
            >
              📋 Copy URL
            </Button>
          </div>
        </div>
      </Card>

      {/* Tabs */}
      <div className="mb-6">
        <div className="border-b border-gray-200">
          <nav className="-mb-px flex space-x-8">
            {tabs.map(tab => (
              <button
                key={tab.id}
                onClick={() => setActiveTab(tab.id)}
                className={`py-2 px-1 border-b-2 font-medium text-sm whitespace-nowrap ${
                  activeTab === tab.id
                    ? 'border-blue-500 text-blue-600'
                    : 'border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300'
                }`}
              >
                <span className="mr-2">{tab.icon}</span>
                {tab.label}
              </button>
            ))}
          </nav>
        </div>
      </div>

      {/* Tab Content */}
      <div className="mb-6">{renderTabContent()}</div>
    </div>
  );
};
