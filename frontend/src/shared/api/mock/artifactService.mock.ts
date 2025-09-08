import type {
  ArtifactUploadResponse,
  PresignedUrlResponse,
} from '@/shared/types/openapi.types';

// Mock data para artefactos
const mockArtifacts = [
  {
    id: 'artifact-001',
    name: 'spring-boot-starter-web',
    version: '2.7.0',
    type: 'maven' as const,
    repositoryId: '550e8400-e29b-41d4-a716-446655440001',
    size: 156789,
    checksum: 'sha256:abc123...',
    createdAt: '2024-01-20T10:30:00Z',
    downloadCount: 1234,
    description: 'Spring Boot Web Starter',
  },
  {
    id: 'artifact-002',
    name: 'react',
    version: '18.2.0',
    type: 'npm' as const,
    repositoryId: '550e8400-e29b-41d4-a716-446655440002',
    size: 89765,
    checksum: 'sha256:def456...',
    createdAt: '2024-01-21T14:20:00Z',
    downloadCount: 5678,
    description: 'React library for building user interfaces',
  },
  {
    id: 'artifact-003',
    name: 'requests',
    version: '2.28.1',
    type: 'pypi' as const,
    repositoryId: '550e8400-e29b-41d4-a716-446655440003',
    size: 23456,
    checksum: 'sha256:ghi789...',
    createdAt: '2024-01-22T09:15:00Z',
    downloadCount: 890,
    description: 'Python HTTP library',
  },
];

export const artifactServiceMock = {
  async uploadArtifact(
    repositoryId: string,
    file: File
  ): Promise<ArtifactUploadResponse> {
    await new Promise(resolve => setTimeout(resolve, 1000)); // Simular upload

    return {
      id: `artifact-${Date.now()}`,
      status: Math.random() > 0.1 ? 'accepted' : 'duplicate',
      repositoryId,
    };
  },

  async getPresignedUrl(
    repositoryId: string,
    filename: string
  ): Promise<PresignedUrlResponse> {
    await new Promise(resolve => setTimeout(resolve, 500));

    return {
      url: `https://presigned.example.com/upload/${repositoryId}/${filename}`,
      expiresAt: new Date(Date.now() + 3600000).toISOString(), // 1 hora
    };
  },

  async getArtifacts(repositoryId?: string) {
    await new Promise(resolve => setTimeout(resolve, 600));

    if (repositoryId) {
      return mockArtifacts.filter(
        artifact => artifact.repositoryId === repositoryId
      );
    }

    return mockArtifacts;
  },

  async getArtifact(id: string) {
    await new Promise(resolve => setTimeout(resolve, 300));

    return mockArtifacts.find(artifact => artifact.id === id) || null;
  },

  async deleteArtifact(id: string): Promise<void> {
    await new Promise(resolve => setTimeout(resolve, 400));

    const index = mockArtifacts.findIndex(artifact => artifact.id === id);
    if (index === -1) throw new Error('Artifact not found');

    mockArtifacts.splice(index, 1);
  },
};
