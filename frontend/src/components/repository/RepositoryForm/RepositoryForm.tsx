import React, { useEffect } from 'react';
import { useForm, Controller } from 'react-hook-form';
import { Input } from '@/components/ui/Input';
import { Button } from '@/components/ui/Button';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/Select';
import type {
  Repository,
  NewRepository,
  UpdateRepository,
  RepositoryType,
} from '@/features/repositories/types/repository.types';

interface RepositoryFormProps {
  repository?: Repository;
  onSubmit: (data: NewRepository | UpdateRepository) => void;
  isSubmitting: boolean;
}

const RepositoryForm = ({
  repository,
  onSubmit,
  isSubmitting,
}: RepositoryFormProps) => {
  const {
    register,
    handleSubmit,
    reset,
    control,
    formState: { errors },
  } = useForm<NewRepository | UpdateRepository>({
    defaultValues: repository || {
      name: '',
      description: '',
      type: 'maven',
      isPublic: true,
      configuration: {},
    },
  });

  useEffect(() => {
    if (repository) {
      reset(repository);
    }
  }, [repository, reset]);

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

  return (
    <form onSubmit={handleSubmit(onSubmit)} className="space-y-4">
      <div className="space-y-2">
        <label htmlFor="name" className="block text-sm font-medium text-gray-700">
          Repository Name *
        </label>
        <Input
          id="name"
          {...register('name', { 
            required: 'Repository name is required',
            minLength: {
              value: 3,
              message: 'Repository name must be at least 3 characters'
            },
            pattern: {
              value: /^[a-zA-Z0-9][a-zA-Z0-9-_]*[a-zA-Z0-9]$/,
              message: 'Repository name can only contain letters, numbers, hyphens, and underscores'
            }
          })}
          placeholder="my-repository"
          disabled={isSubmitting}
          className={errors.name ? 'border-red-300' : ''}
        />
        {errors.name && (
          <p className="mt-1 text-sm text-red-600">{errors.name.message}</p>
        )}
      </div>

      <div className="space-y-2">
        <label
          htmlFor="description"
          className="block text-sm font-medium text-gray-700"
        >
          Description *
        </label>
        <textarea
          id="description"
          {...register('description', {
            required: 'Description is required',
            minLength: {
              value: 10,
              message: 'Description must be at least 10 characters',
            },
          })}
          placeholder="A brief description of your repository..."
          rows={3}
          disabled={isSubmitting}
          className={`w-full px-3 py-2 border rounded-md shadow-sm focus:outline-none focus:ring-blue-500 focus:border-blue-500 ${
            errors.description ? 'border-red-300' : 'border-gray-300'
          }`}
        />
        {errors.description && (
          <p className="mt-1 text-sm text-red-600">{errors.description.message}</p>
        )}
      </div>

      <div className="space-y-2">
        <label className="block text-sm font-medium text-gray-700">Type *</label>
        <Controller
          name="type"
          control={control}
          rules={{ required: 'Repository type is required' }}
          render={({ field }) => (
            <Select onValueChange={field.onChange} defaultValue={field.value}>
              <SelectTrigger
                className={errors.type ? 'border-red-300' : 'border-gray-300'}
              >
                <SelectValue placeholder="Select a repository type" />
              </SelectTrigger>
              <SelectContent>
                {repositoryTypes.map(type => (
                  <SelectItem key={type.value} value={type.value}>
                    <div className="flex items-center gap-2">
                      <span>{type.label}</span>
                      <span className="text-xs text-gray-500">
                        {type.description}
                      </span>
                    </div>
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          )}
        />
        {errors.type && (
          <p className="mt-1 text-sm text-red-600">{errors.type.message}</p>
        )}
      </div>

      <div className="space-y-2">
        <label className="block text-sm font-medium text-gray-700">
          Visibility
        </label>
        <Controller
          name="isPublic"
          control={control}
          render={({ field }) => (
            <Select
              onValueChange={value => field.onChange(value === 'true')}
              defaultValue={String(field.value)}
            >
              <SelectTrigger>
                <SelectValue placeholder="Select visibility" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="true">Public</SelectItem>
                <SelectItem value="false">Private</SelectItem>
              </SelectContent>
            </Select>
          )}
        />
      </div>

      <div className="flex justify-end gap-2">
        <Button
          type="button"
          variant="ghost"
          onClick={() => reset()}
          disabled={isSubmitting}
        >
          Reset
        </Button>
        <Button type="submit" disabled={isSubmitting}>
          {isSubmitting ? 'Saving...' : 'Save Changes'}
        </Button>
      </div>
    </form>
  );
};

export { RepositoryForm };