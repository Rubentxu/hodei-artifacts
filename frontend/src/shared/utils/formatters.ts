import type { RepositoryType } from '@/shared/types';

export const formatSize = (bytes: number): string => {
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let size = bytes;
  let unitIndex = 0;

  while (size >= 1024 && unitIndex < units.length - 1) {
    size /= 1024;
    unitIndex++;
  }

  return `${Math.round(size * 100) / 100} ${units[unitIndex]}`;
};

export const formatLastUpdated = (date: string): string => {
  const now = new Date();
  const updated = new Date(date);
  const diffInMs = now.getTime() - updated.getTime();
  const diffInDays = Math.floor(diffInMs / (1000 * 60 * 60 * 24));

  if (diffInDays === 0) return 'Today';
  if (diffInDays === 1) return 'Yesterday';
  if (diffInDays < 7) return `${diffInDays} days ago`;
  if (diffInDays < 30) return `${Math.floor(diffInDays / 7)} weeks ago`;
  if (diffInDays < 365) return `${Math.floor(diffInDays / 30)} months ago`;
  return `${Math.floor(diffInDays / 365)} years ago`;
};

export const getRepositoryIcon = (type: RepositoryType): string => {
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

export const getRepositoryTypeColor = (type: RepositoryType): string => {
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

export const getVisibilityBadge = (isPublic: boolean): string => {
  return isPublic
    ? 'bg-green-100 text-green-800'
    : 'bg-yellow-100 text-yellow-800';
};
