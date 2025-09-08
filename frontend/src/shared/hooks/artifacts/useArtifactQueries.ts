/**
 * Hooks de React Query para consultas de artefactos
 * Sigue principios SOLID y separación de responsabilidades
 */

import { useQuery } from '@tanstack/react-query';
import type {
  ArtifactUploadResponse,
  PresignedUrlResponse,
} from '@/shared/types/openapi-generated.types';
import { useArtifactService } from './useArtifactService';
import { ARTIFACT_QUERY_KEYS } from './artifactQueryKeys';

/**
 * Hook to get artifact information
 * Single Responsibility Principle: Only handles artifact queries
 */
export function useArtifactInfo(
  artifactId: string,
  presigned: boolean = false
) {
  const artifactService = useArtifactService();

  return useQuery<Blob | PresignedUrlResponse, Error>({
    queryKey: ARTIFACT_QUERY_KEYS.ARTIFACT(artifactId, presigned),
    queryFn: () => artifactService.getArtifact(artifactId, presigned),
    enabled: !!artifactId, // Only execute if there's a valid ID
    staleTime: 5 * 60 * 1000, // 5 minutes
    gcTime: 10 * 60 * 1000, // 10 minutes
    retry: 2,
  });
}

/**
 * Hook to get a presigned URL for an artifact
 * Single Responsibility Principle: Only handles presigned URLs
 */
export function useArtifactPresignedUrl(artifactId: string) {
  const artifactService = useArtifactService();

  return useQuery<PresignedUrlResponse, Error>({
    queryKey: ARTIFACT_QUERY_KEYS.PRESIGNED_URL(artifactId),
    queryFn: () => artifactService.getPresignedUrl(artifactId),
    enabled: !!artifactId, // Only execute if there's a valid ID
    staleTime: 10 * 60 * 1000, // 10 minutes
    gcTime: 30 * 60 * 1000, // 30 minutes
    retry: 1,
  });
}

/**
 * Hook to download an artifact
 * Single Responsibility Principle: Only handles downloads
 */
export function useArtifactDownload(artifactId: string) {
  const artifactService = useArtifactService();

  return useQuery<Blob, Error>({
    queryKey: ARTIFACT_QUERY_KEYS.DOWNLOAD(artifactId),
    queryFn: () => artifactService.downloadArtifact(artifactId),
    enabled: !!artifactId, // Only execute if there's a valid ID
    staleTime: 5 * 60 * 1000, // 5 minutes
    gcTime: 15 * 60 * 1000, // 15 minutes
    retry: 2,
  });
}
