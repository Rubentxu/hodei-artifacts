// Utility functions for formatting repository data

import type { Repository, RepositoryType } from '@/shared/types';

/**
 * Get the appropriate icon for a repository type
 * @param type - The repository type
 * @returns A string representing the icon
 */
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

/**
 * Get the appropriate color class for a repository type
 * @param type - The repository type
 * @returns A string representing the color classes
 */
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

/**
 * Get the appropriate color class for a repository visibility
 * @param visibility - The repository visibility ('public' | 'private')
 * @returns A string representing the color classes
 */
export const getVisibilityBadge = (visibility: 'public' | 'private'): string => {
  return visibility === 'public'
    ? 'bg-green-100 text-green-800'
    : 'bg-yellow-100 text-yellow-800';
};

/**
 * Format repository size in a human-readable format
 * @param bytes - The size in bytes
 * @returns A formatted string representing the size
 */
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

/**
 * Format a date to a relative time string
 * @param dateString - The date string to format
 * @returns A formatted string representing the relative time
 */
export const formatLastUpdated = (dateString: string): string => {
  const date = new Date(dateString);
  const now = new Date();
  const diffInHours = Math.floor(
    (now.getTime() - date.getTime()) / (1000 * 60)
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