import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { policyService } from '../services/policyApi';
import { useNotificationStore } from '@/shared/stores';
import type { NewPolicy, UpdatePolicy, Policy } from '../types/policy.types';

const POLICIES_KEY = 'policies';

export const usePolicies = () => {
  const queryClient = useQueryClient();
  const { addNotification } = useNotificationStore();

  const { data: policies, isLoading } = useQuery({
    queryKey: [POLICIES_KEY],
    queryFn: () => policyService.getPolicies(),
  });

  const { mutate: createPolicy, isPending: isCreating } = useMutation({
    mutationFn: (data: NewPolicy) => policyService.createPolicy(data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: [POLICIES_KEY] });
      addNotification({
        type: 'success',
        title: 'Policy Created',
        message: 'A new policy has been created successfully.',
      });
    },
    onError: error => {
      addNotification({
        type: 'error',
        title: 'Creation Failed',
        message: error.message || 'Failed to create the policy.',
      });
    },
  });

  const { mutate: updatePolicy, isPending: isUpdating } = useMutation({
    mutationFn: ({ id, data }: { id: string; data: UpdatePolicy }) =>
      policyService.updatePolicy(id, data),
    onSuccess: updatedPolicy => {
      queryClient.invalidateQueries({ queryKey: [POLICIES_KEY] });
      queryClient.setQueryData([POLICIES_KEY, updatedPolicy.id], updatedPolicy);
      addNotification({
        type: 'success',
        title: 'Policy Updated',
        message: `Policy ${updatedPolicy.name} has been updated successfully.`,
      });
    },
    onError: error => {
      addNotification({
        type: 'error',
        title: 'Update Failed',
        message: error.message || 'Failed to update the policy.',
      });
    },
  });

  const { mutate: deletePolicy, isPending: isDeleting } = useMutation({
    mutationFn: (id: string) => policyService.deletePolicy(id),
    onSuccess: (_, id) => {
      queryClient.setQueryData([POLICIES_KEY], (old: Policy[] | undefined) =>
        old ? old.filter(policy => policy.id !== id) : []
      );
      addNotification({
        type: 'success',
        title: 'Policy Deleted',
        message: 'The policy has been deleted successfully.',
      });
    },
    onError: error => {
      addNotification({
        type: 'error',
        title: 'Deletion Failed',
        message: error.message || 'Failed to delete the policy.',
      });
    },
  });

  return {
    policies,
    isLoading,
    createPolicy,
    isCreating,
    updatePolicy,
    isUpdating,
    deletePolicy,
    isDeleting,
  };
};
