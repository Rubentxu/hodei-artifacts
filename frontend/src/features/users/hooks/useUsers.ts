import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { userService } from '../services/userApi';
import { useNotificationStore } from '@/shared/stores';
import type { NewUser, UpdateUser } from '../types/user.types';

const USERS_KEY = 'users';

export const useUsers = () => {
  const queryClient = useQueryClient();
  const { addNotification } = useNotificationStore();

  const { data: users, isLoading } = useQuery({
    queryKey: [USERS_KEY],
    queryFn: () => userService.getUsers(),
  });

  const { mutate: createUser, isPending: isCreating } = useMutation({
    mutationFn: (data: NewUser) => userService.createUser(data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: [USERS_KEY] });
      addNotification({
        type: 'success',
        title: 'User Created',
        message: 'A new user has been created successfully.',
      });
    },
    onError: error => {
      addNotification({
        type: 'error',
        title: 'Creation Failed',
        message: error.message || 'Failed to create the user.',
      });
    },
  });

  const { mutate: updateUser, isPending: isUpdating } = useMutation({
    mutationFn: ({ id, data }: { id: string; data: UpdateUser }) =>
      userService.updateUser(id, data),
    onSuccess: updatedUser => {
      queryClient.invalidateQueries({ queryKey: [USERS_KEY] });
      queryClient.setQueryData([USERS_KEY, updatedUser.id], updatedUser);
      addNotification({
        type: 'success',
        title: 'User Updated',
        message: `User ${updatedUser.name} has been updated successfully.`,
      });
    },
    onError: error => {
      addNotification({
        type: 'error',
        title: 'Update Failed',
        message: error.message || 'Failed to update the user.',
      });
    },
  });

  return { users, isLoading, createUser, isCreating, updateUser, isUpdating };
};
