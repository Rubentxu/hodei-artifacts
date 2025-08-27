import { useState } from 'react';
import { Link } from 'react-router-dom';
import { Card } from '@/components/ui/Card';
import { Button } from '@/components/ui/Button';
import type { Repository, RepositoryType } from '@/shared/types';

interface RepositoryCardProps {
  repository: Repository;
  onEdit?: (repository: Repository) => void;
  onDelete?: (repository: Repository) => void;
  onToggleVisibility?: (repository: Repository) => void;
}

export const RepositoryCard = ({
  repository,
  onEdit,
  onDelete,
  onToggleVisibility,
}: RepositoryCardProps) => {
  const [showMenu, setShowMenu] = useState(false);

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

  const formatLastUpdated = (dateString: string): string => {
    const date = new Date(dateString);
    const now = new Date();
    const diffInHours = Math.floor(
      (now.getTime() - date.getTime()) / (1000 * 60 * 60)
    );

    if (diffInHours < 1) {
      return 'just now';
    } else if (diffInHours < 24) {
      return `${diffInHours}h ago`;
    } else if (diffInHours < 168) {
      return `${Math.floor(diffInHours / 24)}d ago`;
    } else {
      return date.toLocaleDateString();
    }
  };

  const copyToClipboard = async (text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      // TODO: Show toast notification
    } catch (error) {
      console.warn('Failed to copy to clipboard:', error);
    }
  };

  const handleAction = (action: string) => {
    setShowMenu(false);

    switch (action) {
      case 'edit':
        onEdit?.(repository);
        break;
      case 'delete':
        onDelete?.(repository);
        break;
      case 'toggle-visibility':
        onToggleVisibility?.(repository);
        break;
      case 'copy-url':
        copyToClipboard(repository.url);
        break;
      default:
        break;
    }
  };

  return (
    <Card className="p-6 hover:shadow-lg transition-all duration-200 border border-gray-200 hover:border-gray-300">
      <div className="flex items-start justify-between">
        <div className="flex-1">
          <div className="flex items-center gap-3 mb-3">
            <span className="text-2xl">
              {getRepositoryIcon(repository.type)}
            </span>
            <div className="flex-1">
              <div className="flex items-center gap-2 mb-1">
                <Link
                  to={`/repositories/${repository.id}`}
                  className="text-xl font-semibold text-gray-900 hover:text-blue-600 transition-colors"
                >
                  {repository.name}
                </Link>
                <span
                  className={`inline-flex items-center px-2 py-1 rounded-full text-xs font-medium ${getRepositoryTypeColor(repository.type)}`}
                >
                  {repository.type.toUpperCase()}
                </span>
                <span
                  className={`inline-flex items-center px-2 py-1 rounded-full text-xs font-medium ${getVisibilityBadge(repository.isPublic)}`}
                >
                  {repository.isPublic ? 'Public' : 'Private'}
                </span>
              </div>
              <p className="text-gray-600 text-sm line-clamp-2">
                {repository.description}
              </p>
            </div>
          </div>

          <div className="flex items-center gap-6 text-sm text-gray-500">
            <span className="flex items-center gap-1">
              <span className="text-blue-500">📊</span>
              {repository.packageCount.toLocaleString()} packages
            </span>
            <span className="flex items-center gap-1">
              <span className="text-purple-500">💾</span>
              {formatSize(repository.size)}
            </span>
            <span className="flex items-center gap-1">
              <span className="text-green-500">🕒</span>
              Updated {formatLastUpdated(repository.lastUpdated)}
            </span>
          </div>

          {/* Quick Actions */}
          <div className="flex items-center gap-2 mt-4">
            <Button
              variant="outline"
              size="sm"
              onClick={() => copyToClipboard(repository.url)}
              className="text-xs"
            >
              📋 Copy URL
            </Button>
            <Link
              to={`/repositories/${repository.id}`}
              className="inline-flex items-center px-3 py-1 border border-transparent text-xs font-medium rounded-md text-blue-600 bg-blue-50 hover:bg-blue-100 transition-colors"
            >
              👁️ View Details
            </Link>
          </div>
        </div>

        {/* Actions Menu */}
        <div className="relative ml-4">
          <button
            onClick={() => setShowMenu(!showMenu)}
            className="p-2 text-gray-400 hover:text-gray-600 rounded-lg hover:bg-gray-100 transition-colors"
          >
            <svg
              className="w-5 h-5"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d="M12 5v.01M12 12v.01M12 19v.01M12 6a1 1 0 110-2 1 1 0 010 2zm0 7a1 1 0 110-2 1 1 0 010 2zm0 7a1 1 0 110-2 1 1 0 010 2z"
              />
            </svg>
          </button>

          {showMenu && (
            <>
              {/* Backdrop */}
              <div
                className="fixed inset-0 z-10"
                onClick={() => setShowMenu(false)}
              />

              {/* Menu */}
              <div className="absolute right-0 top-full mt-1 w-48 bg-white rounded-lg shadow-lg border border-gray-200 py-1 z-20">
                <button
                  onClick={() => handleAction('edit')}
                  className="w-full text-left px-4 py-2 text-sm text-gray-700 hover:bg-gray-100 flex items-center gap-2"
                >
                  ✏️ Edit Repository
                </button>
                <button
                  onClick={() => handleAction('copy-url')}
                  className="w-full text-left px-4 py-2 text-sm text-gray-700 hover:bg-gray-100 flex items-center gap-2"
                >
                  📋 Copy URL
                </button>
                <button
                  onClick={() => handleAction('toggle-visibility')}
                  className="w-full text-left px-4 py-2 text-sm text-gray-700 hover:bg-gray-100 flex items-center gap-2"
                >
                  {repository.isPublic ? '🔒' : '🔓'}
                  Make {repository.isPublic ? 'Private' : 'Public'}
                </button>
                <hr className="my-1 border-gray-200" />
                <button
                  onClick={() => handleAction('delete')}
                  className="w-full text-left px-4 py-2 text-sm text-red-600 hover:bg-red-50 flex items-center gap-2"
                >
                  🗑️ Delete Repository
                </button>
              </div>
            </>
          )}
        </div>
      </div>
    </Card>
  );
};
