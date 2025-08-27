import { useState } from 'react';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { Spinner } from '@/components/ui/Spinner';
import { useCreateRepository } from '@/shared/hooks/repositories';
import type { RepositoryType, CreateRepositoryRequest } from '@/shared/types';

interface CreateRepositoryModalProps {
  isOpen: boolean;
  onClose: () => void;
  onSuccess?: () => void;
}

export const CreateRepositoryModal = ({
  isOpen,
  onClose,
  onSuccess,
}: CreateRepositoryModalProps) => {
  const [formData, setFormData] = useState<CreateRepositoryRequest>({
    name: '',
    description: '',
    type: 'maven',
    isPublic: true,
    configuration: {},
  });

  const [errors, setErrors] = useState<Record<string, string>>({});

  const createRepositoryMutation = useCreateRepository();

  if (!isOpen) return null;

  const repositoryTypes: {
    value: RepositoryType;
    label: string;
    description: string;
  }[] = [
    {
      value: 'maven',
      label: 'Maven',
      description: 'Java artifacts and dependencies',
    },
    { value: 'npm', label: 'npm', description: 'JavaScript/Node.js packages' },
    {
      value: 'pypi',
      label: 'PyPI',
      description: 'Python packages and libraries',
    },
    {
      value: 'docker',
      label: 'Docker',
      description: 'Container images and manifests',
    },
  ];

  const validateForm = (): boolean => {
    const newErrors: Record<string, string> = {};

    if (!formData.name.trim()) {
      newErrors.name = 'Repository name is required';
    } else if (formData.name.length < 3) {
      newErrors.name = 'Repository name must be at least 3 characters';
    } else if (!/^[a-zA-Z0-9][a-zA-Z0-9-_]*[a-zA-Z0-9]$/.test(formData.name)) {
      newErrors.name =
        'Repository name can only contain letters, numbers, hyphens, and underscores';
    }

    if (!formData.description.trim()) {
      newErrors.description = 'Description is required';
    } else if (formData.description.length < 10) {
      newErrors.description = 'Description must be at least 10 characters';
    }

    if (!formData.type) {
      newErrors.type = 'Repository type is required';
    }

    setErrors(newErrors);
    return Object.keys(newErrors).length === 0;
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();

    if (!validateForm()) {
      return;
    }

    try {
      await createRepositoryMutation.mutateAsync(formData);

      // Reset form
      setFormData({
        name: '',
        description: '',
        type: 'maven',
        isPublic: true,
        configuration: {},
      });
      setErrors({});

      onSuccess?.();
      onClose();
    } catch (error) {
      console.error('Failed to create repository:', error);
      setErrors({
        submit: 'Failed to create repository. Please try again.',
      });
    }
  };

  const handleInputChange = (
    field: keyof CreateRepositoryRequest,
    value: any
  ) => {
    setFormData(prev => ({ ...prev, [field]: value }));

    // Clear error when user starts typing
    if (errors[field]) {
      setErrors(prev => {
        const newErrors = { ...prev };
        delete newErrors[field];
        return newErrors;
      });
    }
  };

  const handleClose = () => {
    if (!createRepositoryMutation.isPending) {
      setFormData({
        name: '',
        description: '',
        type: 'maven',
        isPublic: true,
        configuration: {},
      });
      setErrors({});
      onClose();
    }
  };

  return (
    <div className="fixed inset-0 z-50 overflow-y-auto">
      {/* Backdrop */}
      <div
        className="fixed inset-0 bg-black bg-opacity-50 transition-opacity"
        onClick={handleClose}
      />

      {/* Modal */}
      <div className="flex min-h-full items-center justify-center p-4">
        <div className="relative bg-white rounded-lg shadow-xl max-w-md w-full max-h-[90vh] overflow-y-auto">
          <form onSubmit={handleSubmit}>
            {/* Header */}
            <div className="flex items-center justify-between p-6 border-b border-gray-200">
              <h3 className="text-lg font-semibold text-gray-900">
                Create New Repository
              </h3>
              <button
                type="button"
                onClick={handleClose}
                disabled={createRepositoryMutation.isPending}
                className="text-gray-400 hover:text-gray-600 disabled:opacity-50"
              >
                <svg
                  className="w-6 h-6"
                  fill="none"
                  stroke="currentColor"
                  viewBox="0 0 24 24"
                >
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth={2}
                    d="M6 18L18 6M6 6l12 12"
                  />
                </svg>
              </button>
            </div>

            {/* Body */}
            <div className="p-6 space-y-4">
              {/* Repository Name */}
              <div>
                <label className="block text-sm font-medium text-gray-700 mb-1">
                  Repository Name *
                </label>
                <Input
                  type="text"
                  value={formData.name}
                  onChange={e => handleInputChange('name', e.target.value)}
                  placeholder="my-repository"
                  disabled={createRepositoryMutation.isPending}
                  className={
                    errors.name
                      ? 'border-red-300 focus:border-red-500 focus:ring-red-500'
                      : ''
                  }
                />
                {errors.name && (
                  <p className="mt-1 text-sm text-red-600">{errors.name}</p>
                )}
              </div>

              {/* Description */}
              <div>
                <label className="block text-sm font-medium text-gray-700 mb-1">
                  Description *
                </label>
                <textarea
                  value={formData.description}
                  onChange={e =>
                    handleInputChange('description', e.target.value)
                  }
                  placeholder="A brief description of your repository..."
                  rows={3}
                  disabled={createRepositoryMutation.isPending}
                  className={`w-full px-3 py-2 border rounded-md shadow-sm focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-blue-500 disabled:bg-gray-50 disabled:text-gray-500 ${
                    errors.description
                      ? 'border-red-300 focus:border-red-500 focus:ring-red-500'
                      : 'border-gray-300'
                  }`}
                />
                {errors.description && (
                  <p className="mt-1 text-sm text-red-600">
                    {errors.description}
                  </p>
                )}
              </div>

              {/* Repository Type */}
              <div>
                <label className="block text-sm font-medium text-gray-700 mb-1">
                  Repository Type *
                </label>
                <div className="space-y-2">
                  {repositoryTypes.map(type => (
                    <label
                      key={type.value}
                      className="flex items-start space-x-3 cursor-pointer"
                    >
                      <input
                        type="radio"
                        name="type"
                        value={type.value}
                        checked={formData.type === type.value}
                        onChange={e =>
                          handleInputChange(
                            'type',
                            e.target.value as RepositoryType
                          )
                        }
                        disabled={createRepositoryMutation.isPending}
                        className="mt-1 h-4 w-4 text-blue-600 focus:ring-blue-500 border-gray-300"
                      />
                      <div className="flex-1">
                        <div className="text-sm font-medium text-gray-900">
                          {type.label}
                        </div>
                        <div className="text-sm text-gray-500">
                          {type.description}
                        </div>
                      </div>
                    </label>
                  ))}
                </div>
                {errors.type && (
                  <p className="mt-1 text-sm text-red-600">{errors.type}</p>
                )}
              </div>

              {/* Visibility */}
              <div>
                <label className="block text-sm font-medium text-gray-700 mb-2">
                  Visibility
                </label>
                <div className="space-y-2">
                  <label className="flex items-start space-x-3 cursor-pointer">
                    <input
                      type="radio"
                      name="visibility"
                      checked={formData.isPublic}
                      onChange={() => handleInputChange('isPublic', true)}
                      disabled={createRepositoryMutation.isPending}
                      className="mt-1 h-4 w-4 text-blue-600 focus:ring-blue-500 border-gray-300"
                    />
                    <div className="flex-1">
                      <div className="text-sm font-medium text-gray-900">
                        🔓 Public
                      </div>
                      <div className="text-sm text-gray-500">
                        Anyone can view and download packages
                      </div>
                    </div>
                  </label>
                  <label className="flex items-start space-x-3 cursor-pointer">
                    <input
                      type="radio"
                      name="visibility"
                      checked={!formData.isPublic}
                      onChange={() => handleInputChange('isPublic', false)}
                      disabled={createRepositoryMutation.isPending}
                      className="mt-1 h-4 w-4 text-blue-600 focus:ring-blue-500 border-gray-300"
                    />
                    <div className="flex-1">
                      <div className="text-sm font-medium text-gray-900">
                        🔒 Private
                      </div>
                      <div className="text-sm text-gray-500">
                        Only authorized users can access
                      </div>
                    </div>
                  </label>
                </div>
              </div>

              {/* Submit Error */}
              {errors.submit && (
                <div className="bg-red-50 border border-red-200 rounded-md p-3">
                  <p className="text-sm text-red-600">{errors.submit}</p>
                </div>
              )}
            </div>

            {/* Footer */}
            <div className="flex items-center justify-end gap-3 p-6 border-t border-gray-200">
              <Button
                type="button"
                variant="outline"
                onClick={handleClose}
                disabled={createRepositoryMutation.isPending}
              >
                Cancel
              </Button>
              <Button
                type="submit"
                disabled={createRepositoryMutation.isPending}
                className="min-w-[120px]"
              >
                {createRepositoryMutation.isPending ? (
                  <div className="flex items-center gap-2">
                    <Spinner size="sm" variant="white" />
                    Creating...
                  </div>
                ) : (
                  'Create Repository'
                )}
              </Button>
            </div>
          </form>
        </div>
      </div>
    </div>
  );
};
