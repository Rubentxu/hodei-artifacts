/**
 * Hooks de React Query para mutaciones de repositorios
 * Sigue principios SOLID y separación de responsabilidades
 */

import { useMutation, useQueryClient } from '@tanstack/react-query';
import type { 
  Repository, 
  CreateRepositoryRequest, 
  UpdateRepositoryRequest 
} from '@/shared/types';
import { useRepositoryService } from './useRepositoryService';
import { QUERY_KEYS } from '@/shared/constants';
import { useNotifications } from '@/shared/stores/ui.store';

/**
 * Hook para crear un nuevo repositorio
 * Principio de Responsabilidad Única: Solo maneja la creación
 */
export function useCreateRepository() {
  const queryClient = useQueryClient();
  const repositoryService = useRepositoryService();
  const { showSuccess, showError } = useNotifications();

  return useMutation<Repository, Error, CreateRepositoryRequest>({
    mutationFn: (data) => repositoryService.crearRepositorio(data),
    onSuccess: (newRepository) => {
      // Invalidar caché de lista de repositorios
      queryClient.invalidateQueries({ queryKey: QUERY_KEYS.REPOSITORIES });
      
      // Agregar el nuevo repositorio al caché optimistamente
      queryClient.setQueryData(
        QUERY_KEYS.REPOSITORY(newRepository.id),
        { data: newRepository }
      );

      showSuccess('Repositorio creado', `El repositorio "${newRepository.name}" se ha creado exitosamente`);
    },
    onError: (error) => {
      showError('Error al crear repositorio', error.message);
    },
  });
}

/**
 * Hook para actualizar un repositorio existente
 * Principio de Responsabilidad Única: Solo maneja la actualización
 */
export function useUpdateRepository() {
  const queryClient = useQueryClient();
  const repositoryService = useRepositoryService();
  const { showSuccess, showError } = useNotifications();

  return useMutation<Repository, Error, { id: string; data: UpdateRepositoryRequest }>({
    mutationFn: ({ id, data }) => repositoryService.actualizarRepositorio(id, data),
    onSuccess: (updatedRepository, variables) => {
      // Actualizar el repositorio en el caché
      queryClient.setQueryData(
        QUERY_KEYS.REPOSITORY(variables.id),
        { data: updatedRepository }
      );

      // Invalidar lista de repositorios si es necesario
      queryClient.invalidateQueries({ queryKey: QUERY_KEYS.REPOSITORIES });

      showSuccess('Repositorio actualizado', `El repositorio "${updatedRepository.name}" se ha actualizado exitosamente`);
    },
    onError: (error, variables) => {
      showError('Error al actualizar repositorio', error.message);
    },
  });
}

/**
 * Hook para eliminar un repositorio
 * Principio de Responsabilidad Única: Solo maneja la eliminación
 */
export function useDeleteRepository() {
  const queryClient = useQueryClient();
  const repositoryService = useRepositoryService();
  const { showSuccess, showError } = useNotifications();

  return useMutation<void, Error, string>({
    mutationFn: (id) => repositoryService.eliminarRepositorio(id),
    onSuccess: (_, deletedId) => {
      // Invalidar caché de lista de repositorios
      queryClient.invalidateQueries({ queryKey: QUERY_KEYS.REPOSITORIES });
      
      // Eliminar el repositorio del caché
      queryClient.removeQueries({ queryKey: QUERY_KEYS.REPOSITORY(deletedId) });

      showSuccess('Repositorio eliminado', 'El repositorio se ha eliminado exitosamente');
    },
    onError: (error) => {
      showError('Error al eliminar repositorio', error.message);
    },
  });
}