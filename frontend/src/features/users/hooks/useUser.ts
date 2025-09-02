import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { userService } from '../services/userApi';
import { useNotifications } from '@/shared/stores/ui.store';
import type { UserProfile, UpdateUserProfile } from '../types/user.types';

const USER_PROFILE_KEY = 'userProfile';

export const useUser = () => {
  const queryClient = useQueryClient();
  const { showSuccess, showError } = useNotifications();

  const { data: user, isLoading } = useQuery({
    queryKey: [USER_PROFILE_KEY],
    queryFn: () => userService.getMyProfile(),
    staleTime: 5 * 60 * 1000, // 5 minutes
  });

  const { mutate: updateUser, isPending: isUpdating } = useMutation({
    mutationFn: (data: UpdateUserProfile) => userService.updateMyProfile(data),
    onSuccess: updatedUser => {
      queryClient.setQueryData([USER_PROFILE_KEY], updatedUser);
      showSuccess(
        'Profile Updated',
        'Your profile has been updated successfully.'
      );
    },
    onError: error => {
      showError(
        'Update Failed',
        error.message || 'Failed to update your profile.'
      );
    },
  });

  return { user, isLoading, updateUser, isUpdating };
};
