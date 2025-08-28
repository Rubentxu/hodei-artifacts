import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { tokenService } from '../services/tokenApi';
import { useNotificationStore } from '@/shared/stores/notificationStore';
import type { NewApiToken, ApiToken } from '../types/token.types';

const TOKENS_KEY = 'tokens';

export const useTokens = () => {
  const queryClient = useQueryClient();
  const { addNotification } = useNotificationStore();

  const { data: tokens, isLoading } = useQuery({
    queryKey: [TOKENS_KEY],
    queryFn: () => tokenService.getTokens(),
  });

  const { mutate: createToken, isPending: isCreating } = useMutation({
    mutationFn: (data: NewApiToken) => tokenService.createToken(data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: [TOKENS_KEY] });
      addNotification({
        type: 'success',
        title: 'Token Created',
        message: 'A new API token has been created successfully.',
      });
    },
    onError: error => {
      addNotification({
        type: 'error',
        title: 'Creation Failed',
        message: error.message || 'Failed to create the token.',
      });
    },
  });

  const { mutate: revokeToken, isPending: isRevoking } = useMutation({
    mutationFn: (id: string) => tokenService.revokeToken(id),
    onSuccess: (_, id) => {
      queryClient.setQueryData([TOKENS_KEY], (old: ApiToken[] | undefined) =>
        old ? old.filter(token => token.id !== id) : []
      );
      addNotification({
        type: 'success',
        title: 'Token Revoked',
        message: 'The API token has been revoked successfully.',
      });
    },
    onError: error => {
      addNotification({
        type: 'error',
        title: 'Revocation Failed',
        message: error.message || 'Failed to revoke the token.',
      });
    },
  });

  return {
    tokens,
    isLoading,
    createToken,
    isCreating,
    revokeToken,
    isRevoking,
  };
};
