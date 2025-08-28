import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { userService } from '../services/userApi';
import { useNotificationStore } from '@/shared/stores/notificationStore';
import type { UpdateUserProfile } from '../types/user.types';

const USER_KEY = 'user';

export const useUser = () => {
  const queryClient = useQueryClient();
  const { addNotification } = useNotificationStore();

  const { data: user, isLoading } = useQuery({
    queryKey: [USER_KEY, 'me'],
    queryFn: () => userService.getMyProfile(),
  });

  const { mutate: updateUser, isPending: isUpdating } = useMutation({
    mutationFn: (data: UpdateUserProfile) => userService.updateMyProfile(data),
    onSuccess: updatedUser => {
      queryClient.setQueryData([USER_KEY, 'me'], updatedUser);
      addNotification({
        type: 'success',
        title: 'Profile Updated',
        message: 'Your profile has been updated successfully.',
      });
    },
    onError: error => {
      addNotification({
        type: 'error',
        title: 'Update Failed',
        message: error.message || 'Failed to update your profile.',
      });
    },
  });

  return { user, isLoading, updateUser, isUpdating };
};
