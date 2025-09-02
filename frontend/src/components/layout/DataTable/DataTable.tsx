import React from 'react';
import { cn } from '@/shared/utils';
import { Spinner } from '@/components/ui/Spinner';

export interface Column<T> {
  key: keyof T | 'actions';
  title: string;
  sortable?: boolean;
  render?: (value: any, row: T) => React.ReactNode;
  className?: string;
}

export interface DataTableProps<T> {
  data: T[];
  columns: Column<T>[];
  loading?: boolean;
  className?: string;
  onSort?: (field: keyof T, direction: 'asc' | 'desc') => void;
  sortField?: keyof T;
  sortDirection?: 'asc' | 'desc';
}

export function DataTable<T extends Record<string, any>>({
  data,
  columns,
  loading = false,
  className,
  onSort,
  sortField,
  sortDirection,
}: DataTableProps<T>) {
  const handleSort = (column: Column<T>) => {
    if (!column.sortable || !onSort || column.key === 'actions') return;

    const newDirection =
      sortField === column.key && sortDirection === 'asc' ? 'desc' : 'asc';
    onSort(column.key as keyof T, newDirection);
  };

  const getSortIcon = (column: Column<T>) => {
    if (!column.sortable || column.key === 'actions') return null;

    if (sortField !== column.key) {
      return <span className="text-gray-400">↕️</span>;
    }

    return sortDirection === 'asc' ? (
      <span className="text-blue-500">↑</span>
    ) : (
      <span className="text-blue-500">↓</span>
    );
  };

  if (loading) {
    return (
      <div className="flex items-center justify-center p-8">
        <Spinner size="lg" />
        <span className="ml-2 text-gray-600">Loading...</span>
      </div>
    );
  }

  if (!data || data.length === 0) {
    return (
      <div className="text-center py-8 text-gray-500">No data available</div>
    );
  }

  return (
    <div className={cn('overflow-x-auto', className)}>
      <table className="min-w-full divide-y divide-gray-200 dark:divide-gray-700">
        <thead className="bg-gray-50 dark:bg-gray-800">
          <tr>
            {columns.map(column => (
              <th
                key={String(column.key)}
                className={cn(
                  'px-6 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider',
                  column.sortable &&
                    column.key !== 'actions' &&
                    'cursor-pointer hover:bg-gray-100 dark:hover:bg-gray-700',
                  column.className
                )}
                onClick={() => handleSort(column)}
              >
                <div className="flex items-center space-x-1">
                  <span>{column.title}</span>
                  {getSortIcon(column)}
                </div>
              </th>
            ))}
          </tr>
        </thead>
        <tbody className="bg-white dark:bg-gray-900 divide-y divide-gray-200 dark:divide-gray-700">
          {data.map((row, index) => (
            <tr
              key={index}
              className="hover:bg-gray-50 dark:hover:bg-gray-800 transition-colors"
            >
              {columns.map(column => (
                <td
                  key={String(column.key)}
                  className="px-6 py-4 whitespace-nowrap text-sm text-gray-900 dark:text-gray-100"
                >
                  {column.render
                    ? column.render(
                        column.key === 'actions' ? undefined : row[column.key],
                        row
                      )
                    : String(row[column.key] || '')}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export default DataTable;
