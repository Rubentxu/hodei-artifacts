import type {
  SearchResults,
  PackageResult,
  SearchQuery,
} from '@/shared/types/openapi.types';

// Mock data para resultados de búsqueda
const mockSearchResults: PackageResult[] = [
  {
    id: 'package-001',
    name: 'react',
    version: '18.2.0',
    description: 'React is a JavaScript library for building user interfaces.',
    repositoryId: '550e8400-e29b-41d4-a716-446655440002',
    repositoryName: 'npm-public',
    packageType: 'npm',
    createdAt: '2024-01-15T10:30:00Z',
    downloadCount: 1234567,
  },
  {
    id: 'package-002',
    name: 'spring-boot-starter-web',
    version: '2.7.0',
    description: 'Spring Boot Web Starter',
    repositoryId: '550e8400-e29b-41d4-a716-446655440001',
    repositoryName: 'maven-central',
    packageType: 'maven',
    createdAt: '2024-01-16T14:20:00Z',
    downloadCount: 456789,
  },
  {
    id: 'package-003',
    name: 'requests',
    version: '2.28.1',
    description: 'Python HTTP library',
    repositoryId: '550e8400-e29b-41d4-a716-446655440003',
    repositoryName: 'pypi-internal',
    packageType: 'pypi',
    createdAt: '2024-01-17T09:15:00Z',
    downloadCount: 234567,
  },
  {
    id: 'package-004',
    name: 'lodash',
    version: '4.17.21',
    description: 'Lodash modular utilities.',
    repositoryId: '550e8400-e29b-41d4-a716-446655440002',
    repositoryName: 'npm-public',
    packageType: 'npm',
    createdAt: '2024-01-18T16:45:00Z',
    downloadCount: 890123,
  },
  {
    id: 'package-005',
    name: 'junit',
    version: '5.8.2',
    description: 'JUnit 5 testing framework',
    repositoryId: '550e8400-e29b-41d4-a716-446655440001',
    repositoryName: 'maven-central',
    packageType: 'maven',
    createdAt: '2024-01-19T11:30:00Z',
    downloadCount: 345678,
  },
];

export const searchServiceMock = {
  async searchPackages(query: SearchQuery): Promise<SearchResults> {
    await new Promise(resolve => setTimeout(resolve, 800)); // Simular búsqueda

    let results = [...mockSearchResults];

    // Filtrar por query
    if (query.q) {
      const searchTerm = query.q.toLowerCase();
      results = results.filter(
        pkg =>
          pkg.name.toLowerCase().includes(searchTerm) ||
          pkg.description?.toLowerCase().includes(searchTerm)
      );
    }

    // Filtrar por tipo
    if (query.type) {
      results = results.filter(pkg => pkg.packageType === query.type);
    }

    // Filtrar por repositoryId
    if (query.repositoryId) {
      results = results.filter(pkg => pkg.repositoryId === query.repositoryId);
    }

    // Ordenar
    if (query.sortBy) {
      results.sort((a, b) => {
        const aVal = a[query.sortBy as keyof PackageResult];
        const bVal = b[query.sortBy as keyof PackageResult];

        if (typeof aVal === 'string' && typeof bVal === 'string') {
          return query.sortOrder === 'desc'
            ? bVal.localeCompare(aVal)
            : aVal.localeCompare(bVal);
        }

        if (typeof aVal === 'number' && typeof bVal === 'number') {
          return query.sortOrder === 'desc' ? bVal - aVal : aVal - bVal;
        }

        return 0;
      });
    }

    // Paginación
    const page = query.page || 1;
    const limit = query.limit || 10;
    const startIndex = (page - 1) * limit;
    const endIndex = startIndex + limit;

    const paginatedResults = results.slice(startIndex, endIndex);

    return {
      total: results.length,
      items: paginatedResults,
    };
  },

  async getSearchSuggestions(query: string): Promise<string[]> {
    await new Promise(resolve => setTimeout(resolve, 300));

    const allPackages = mockSearchResults.map(pkg => pkg.name);
    const suggestions = allPackages
      .filter(name => name.toLowerCase().startsWith(query.toLowerCase()))
      .slice(0, 5);

    return suggestions;
  },

  async getPopularPackages(limit: number = 10): Promise<PackageResult[]> {
    await new Promise(resolve => setTimeout(resolve, 400));

    return [...mockSearchResults]
      .sort((a, b) => b.downloadCount - a.downloadCount)
      .slice(0, limit);
  },

  async getRecentPackages(limit: number = 10): Promise<PackageResult[]> {
    await new Promise(resolve => setTimeout(resolve, 400));

    return [...mockSearchResults]
      .sort(
        (a, b) =>
          new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime()
      )
      .slice(0, limit);
  },
};
