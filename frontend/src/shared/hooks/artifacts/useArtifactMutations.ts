/**
 * Hooks de React Query para mutaciones de artefactos
 * Sigue principios SOLID y separación de responsabilidades
 */

import { useMutation, useQueryClient } from '@tanstack/react-query';
import type {
  ArtifactUploadResponse,
  UploadArtifactBody,
} from '@/shared/types/openapi-generated.types';
import { useArtifactService } from './useArtifactService';
import { ARTIFACT_QUERY_KEYS } from './artifactQueryKeys';

/**
 * Hook to upload an artifact
 * Single Responsibility Principle: Only handles artifact uploads
 */
export function useUploadArtifact() {
  const queryClient = useQueryClient();
  const artifactService = useArtifactService();

  return useMutation<ArtifactUploadResponse, Error, UploadArtifactBody>({
    mutationFn: async (body: UploadArtifactBody) => {
      return await artifactService.uploadArtifact(
        body.file,
        JSON.parse(body.metadata)
      );
    },
    onSuccess: data => {
      // Invalidate artifact cache
      queryClient.invalidateQueries({ queryKey: ARTIFACT_QUERY_KEYS.LIST() });

      console.log('Artifact uploaded successfully:', data);
    },
    onError: error => {
      console.error('Error uploading artifact:', error);
    },
  });
}

/**
 * Hook to validate a file before uploading
 * Single Responsibility Principle: Only handles file validation
 */
export function useValidateArtifact() {
  const artifactService = useArtifactService();

  return useMutation<{ valid: boolean; errors: string[] }, Error, File>({
    mutationFn: async (file: File) => {
      return artifactService.validateFile(file);
    },
    onError: error => {
      console.error('Error validating file:', error);
    },
  });
}

/**
 * Hook to get package type of a file
 * Single Responsibility Principle: Only handles type analysis
 */
export function useAnalyzePackageType() {
  const artifactService = useArtifactService();

  return useMutation<'maven' | 'npm' | 'pypi' | 'unknown', Error, string>({
    mutationFn: async (filename: string) => {
      return artifactService.getPackageType(filename);
    },
    onError: error => {
      console.error('Error analyzing package type:', error);
    },
  });
}

/**
 * Hook to generate metadata for an artifact
 * Single Responsibility Principle: Only handles metadata generation
 */
export function useGenerateArtifactMetadata() {
  const artifactService = useArtifactService();

  return useMutation<
    Record<string, any>,
    Error,
    { file: File; type?: 'maven' | 'npm' | 'pypi' }
  >({
    mutationFn: async ({ file, type }) => {
      return artifactService.generateMetadata(file, type);
    },
    onError: error => {
      console.error('Error generating metadata:', error);
    },
  });
}
