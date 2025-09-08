import React, { useState, useMemo } from 'react';
import { ChevronUp, ChevronDown, Search, Filter, Download, Eye, Edit, Trash2, MoreVertical } from 'lucide-react';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { Card } from '@/components/ui/Card';
import { Badge } from '@/components/ui/Badge';
import { cn } from '@/shared/utils/cn';

export interface DataTableColumn<T> {
  key: keyof T;
  header: string;
  sortable?: boolean;
  filterable?: boolean;
  render?: (value: T[keyof T], row: T) => React.ReactNode;
  width?: string;
  align?: 'left' | 'center' | 'right';
}

export interface DataTableAction<T> {
  label: string;
  icon?: React.ReactNode;
  onClick: (row: T) => void;
  variant?: 'outline' | 'ghost' | 'primary' | 'secondary' | 'destructive';
  color?: 'default' | 'primary' | 'success' | 'danger' | 'warning';
  showInMenu?: boolean;
}

interface DataTableEnhancedProps<T> {
  data: T[];
  columns: DataTableColumn<T>[];
  actions?: DataTableAction<T>[];
  title?: string;
  subtitle?: string;
  searchable?: boolean;
  filterable?: boolean;
  sortable?: boolean;
  selectable?: boolean;
  exportable?: boolean;
  pagination?: boolean;
  pageSize?: number;
  loading?: boolean;
  emptyMessage?: string;
  className?: string;
}

export function DataTableEnhanced<T extends Record<string, any>>({
  data,
  columns,
  actions = [],
  title,
  subtitle,
  searchable = true,
  filterable = true,
  sortable = true,
  selectable = false,
  exportable = true,
  pagination = true,
  pageSize = 10,
  loading = false,
  emptyMessage = "No data available",
  className
}: DataTableEnhancedProps<T>) {
  const [searchTerm, setSearchTerm] = useState('');
  const [sortConfig, setSortConfig] = useState<{
    key: keyof T | null;
    direction: 'asc' | 'desc';
  }>({ key: null, direction: 'asc' });
  const [selectedRows, setSelectedRows] = useState<Set<string>>(new Set());
  const [currentPage, setCurrentPage] = useState(1);
  const [showFilters, setShowFilters] = useState(false);
  const [columnFilters, setColumnFilters] = useState<Record<string, string>>({});

  // Procesar datos con búsqueda, ordenamiento y filtros
  const processedData = useMemo(() => {
    let filteredData = [...data];

    // Búsqueda global
    if (searchTerm) {
      filteredData = filteredData.filter(row =>
        columns.some(column => {
          const value = row[column.key];
          return value?.toString().toLowerCase().includes(searchTerm.toLowerCase());
        })
      );
    }

    // Filtros por columna
    Object.entries(columnFilters).forEach(([key, filterValue]) => {
      if (filterValue) {
        filteredData = filteredData.filter(row => {
          const value = row[key];
          return value?.toString().toLowerCase().includes(filterValue.toLowerCase());
        });
      }
    });

    // Ordenamiento
    if (sortConfig.key) {
      filteredData.sort((a, b) => {
        const aValue = a[sortConfig.key!];
        const bValue = b[sortConfig.key!];
        
        if (aValue === null || aValue === undefined) return 1;
        if (bValue === null || bValue === undefined) return -1;
        
        if (aValue < bValue) return sortConfig.direction === 'asc' ? -1 : 1;
        if (aValue > bValue) return sortConfig.direction === 'asc' ? 1 : -1;
        return 0;
      });
    }

    return filteredData;
  }, [data, searchTerm, sortConfig, columnFilters, columns]);

  // Paginación
  const paginatedData = useMemo(() => {
    if (!pagination) return processedData;
    
    const startIndex = (currentPage - 1) * pageSize;
    const endIndex = startIndex + pageSize;
    return processedData.slice(startIndex, endIndex);
  }, [processedData, currentPage, pageSize, pagination]);

  const totalPages = Math.ceil(processedData.length / pageSize);

  const handleSort = (column: DataTableColumn<T>) => {
    if (!sortable || !column.sortable) return;

    let direction: 'asc' | 'desc' = 'asc';
    if (sortConfig.key === column.key && sortConfig.direction === 'asc') {
      direction = 'desc';
    }

    setSortConfig({ key: column.key, direction });
  };

  const handleSelectAll = () => {
    if (selectedRows.size === paginatedData.length) {
      setSelectedRows(new Set());
    } else {
      setSelectedRows(new Set(paginatedData.map((_, index) => index.toString())));
    }
  };

  const handleSelectRow = (rowIndex: string) => {
    const newSelected = new Set(selectedRows);
    if (newSelected.has(rowIndex)) {
      newSelected.delete(rowIndex);
    } else {
      newSelected.add(rowIndex);
    }
    setSelectedRows(newSelected);
  };

  const handleColumnFilter = (columnKey: string, value: string) => {
    setColumnFilters(prev => ({
      ...prev,
      [columnKey]: value
    }));
  };

  const handleExport = () => {
    const csvContent = [
      columns.map(col => col.header).join(','),
      ...processedData.map(row => 
        columns.map(col => {
          const value = row[col.key];
          return col.render ? `"${col.render(value, row)}"` : `"${value}"`;
        }).join(',')
      )
    ].join('\n');

    const blob = new Blob([csvContent], { type: 'text/csv' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `${title || 'data'}-${new Date().toISOString().split('T')[0]}.csv`;
    a.click();
    URL.revokeObjectURL(url);
  };

  const getSortIcon = (column: DataTableColumn<T>) => {
    if (!sortable || !column.sortable) return null;
    
    if (sortConfig.key !== column.key) {
      return <ChevronUp className="w-4 h-4 text-gray-400" />;
    }
    
    return sortConfig.direction === 'asc' 
      ? <ChevronUp className="w-4 h-4 text-blue-600" />
      : <ChevronDown className="w-4 h-4 text-blue-600" />;
  };

  if (loading) {
    return (
      <Card className={cn("p-8", className)}>
        <div className="flex items-center justify-center">
          <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-primary-600"></div>
        </div>
      </Card>
    );
  }

  return (
    <Card className={cn("overflow-hidden", className)}>
      {/* Header */}
      {(title || searchable || exportable) && (
        <div className="p-6 border-b border-gray-200">
          <div className="flex items-center justify-between">
            <div>
              {title && <h3 className="text-lg font-semibold text-gray-900">{title}</h3>}
              {subtitle && <p className="text-sm text-gray-600 mt-1">{subtitle}</p>}
            </div>
            
            <div className="flex items-center space-x-2">
              {exportable && (
                <Button
                  variant="outline"
                  size="sm"
                  onClick={handleExport}
                  disabled={processedData.length === 0}
                >
                  <Download className="w-4 h-4 mr-2" />
                  Export
                </Button>
              )}
            </div>
          </div>

          {/* Search and Filters */}
          {searchable && (
            <div className="mt-4 flex items-center space-x-2">
              <div className="relative flex-1 max-w-md">
                <Search className="absolute left-3 top-1/2 transform -translate-y-1/2 text-gray-400 w-4 h-4" />
                <Input
                  type="text"
                  placeholder="Search..."
                  value={searchTerm}
                  onChange={(e) => setSearchTerm(e.target.value)}
                  className="pl-10"
                />
              </div>
              {filterable && (
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => setShowFilters(!showFilters)}
                >
                  <Filter className="w-4 h-4 mr-2" />
                  Filters
                </Button>
              )}
            </div>
          )}
        </div>
      )}

      {/* Column Filters */}
      {showFilters && filterable && (
        <div className="p-4 bg-gray-50 border-b border-gray-200">
          <div className="grid grid-cols-1 md:grid-cols-3 lg:grid-cols-4 gap-4">
            {columns.filter(col => col.filterable).map((column) => (
              <div key={String(column.key)}>
                <label className="block text-sm font-medium text-gray-700 mb-1">
                  {column.header}
                </label>
                <Input
                  type="text"
                  placeholder={`Filter ${column.header}...`}
                  value={columnFilters[String(column.key)] || ''}
                  onChange={(e) => handleColumnFilter(String(column.key), e.target.value)}
                  className="text-sm"
                />
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Table */}
      <div className="overflow-x-auto">
        <table className="min-w-full divide-y divide-gray-200">
          <thead className="bg-gray-50">
            <tr>
              {selectable && (
                <th className="px-6 py-3 text-left">
                  <input
                    type="checkbox"
                    checked={selectedRows.size === paginatedData.length && paginatedData.length > 0}
                    onChange={handleSelectAll}
                    className="rounded border-gray-300 text-primary-600 focus:ring-primary-500"
                  />
                </th>
              )}
              {columns.map((column) => (
                <th
                  key={String(column.key)}
                  className={cn(
                    "px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider",
                    column.align === 'center' && "text-center",
                    column.align === 'right' && "text-right",
                    sortable && column.sortable && "cursor-pointer hover:bg-gray-100"
                  )}
                  onClick={() => handleSort(column)}
                  style={{ width: column.width }}
                >
                  <div className={cn(
                    "flex items-center",
                    column.align === 'center' && "justify-center",
                    column.align === 'right' && "justify-end"
                  )}>
                    {column.header}
                    {sortable && column.sortable && (
                      <span className="ml-1">{getSortIcon(column)}</span>
                    )}
                  </div>
                </th>
              ))}
              {actions.length > 0 && (
                <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                  Actions
                </th>
              )}
            </tr>
          </thead>
          <tbody className="bg-white divide-y divide-gray-200">
            {paginatedData.map((row, index) => (
              <tr
                key={index}
                className={cn(
                  "hover:bg-gray-50 transition-colors",
                  selectedRows.has(index.toString()) && "bg-blue-50"
                )}
              >
                {selectable && (
                  <td className="px-6 py-4 whitespace-nowrap">
                    <input
                      type="checkbox"
                      checked={selectedRows.has(index.toString())}
                      onChange={() => handleSelectRow(index.toString())}
                      className="rounded border-gray-300 text-primary-600 focus:ring-primary-500"
                    />
                  </td>
                )}
                {columns.map((column) => (
                  <td
                    key={String(column.key)}
                    className={cn(
                      "px-6 py-4 whitespace-nowrap text-sm text-gray-900",
                      column.align === 'center' && "text-center",
                      column.align === 'right' && "text-right"
                    )}
                  >
                    {column.render 
                      ? column.render(row[column.key], row)
                      : row[column.key]?.toString() || '-'
                    }
                  </td>
                ))}
                {actions.length > 0 && (
                  <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                    <div className="flex items-center space-x-2">
                      {actions.filter(action => !action.showInMenu).map((action, actionIndex) => (
                        <Button
                          key={actionIndex}
                          variant={action.variant || 'ghost'}
                          size="sm"
                          onClick={() => action.onClick(row)}
                          className={cn(
                            action.color === 'danger' && "text-red-600 hover:text-red-700",
                            action.color === 'success' && "text-green-600 hover:text-green-700",
                            action.color === 'warning' && "text-yellow-600 hover:text-yellow-700"
                          )}
                        >
                          {action.icon}
                        </Button>
                      ))}
                      {actions.some(action => action.showInMenu) && (
                        <div className="relative group">
                          <Button variant="ghost" size="sm">
                            <MoreVertical className="w-4 h-4" />
                          </Button>
                          <div className="absolute right-0 top-full mt-1 w-48 bg-white rounded-md shadow-lg border border-gray-200 opacity-0 invisible group-hover:opacity-100 group-hover:visible transition-all duration-200 z-10">
                            <div className="py-1">
                              {actions.filter(action => action.showInMenu).map((action, actionIndex) => (
                                <button
                                  key={actionIndex}
                                  className="w-full px-4 py-2 text-left text-sm text-gray-700 hover:bg-gray-100 flex items-center space-x-2"
                                  onClick={() => action.onClick(row)}
                                >
                                  {action.icon}
                                  <span>{action.label}</span>
                                </button>
                              ))}
                            </div>
                          </div>
                        </div>
                      )}
                    </div>
                  </td>
                )}
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {/* Empty State */}
      {paginatedData.length === 0 && (
        <div className="text-center py-12">
          <div className="text-gray-400 mb-4">
            <Search className="w-12 h-12 mx-auto" />
          </div>
          <h3 className="text-lg font-medium text-gray-900 mb-2">{emptyMessage}</h3>
          <p className="text-gray-600">
            {searchTerm || Object.keys(columnFilters).length > 0 
              ? "Try adjusting your search or filters"
              : "No data to display"
            }
          </p>
        </div>
      )}

      {/* Pagination */}
      {pagination && totalPages > 1 && (
        <div className="px-6 py-4 border-t border-gray-200 flex items-center justify-between">
          <div className="text-sm text-gray-700">
            Showing {((currentPage - 1) * pageSize) + 1} to {Math.min(currentPage * pageSize, processedData.length)} of {processedData.length} results
          </div>
          <div className="flex items-center space-x-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => setCurrentPage(Math.max(1, currentPage - 1))}
              disabled={currentPage === 1}
            >
              Previous
            </Button>
            <div className="flex items-center space-x-1">
              {Array.from({ length: Math.min(5, totalPages) }, (_, i) => {
                const page = i + 1;
                return (
                  <Button
                    key={page}
                    variant={currentPage === page ? 'primary' : 'outline'}
                    size="sm"
                    onClick={() => setCurrentPage(page)}
                  >
                    {page}
                  </Button>
                );
              })}
              {totalPages > 5 && (
                <>
                  <span className="px-2">...</span>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => setCurrentPage(totalPages)}
                  >
                    {totalPages}
                  </Button>
                </>
              )}
            </div>
            <Button
              variant="outline"
              size="sm"
              onClick={() => setCurrentPage(Math.min(totalPages, currentPage + 1))}
              disabled={currentPage === totalPages}
            >
              Next
            </Button>
          </div>
        </div>
      )}
    </Card>
  );
}