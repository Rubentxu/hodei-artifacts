import { InputHTMLAttributes, forwardRef } from 'react';
import { cn } from '@/shared/utils';

export interface InputProps extends InputHTMLAttributes<HTMLInputElement> {
  variant?: 'default' | 'error';
  size?: 'sm' | 'md' | 'lg';
  leftIcon?: React.ReactNode;
  rightIcon?: React.ReactNode;
  label?: string;
  error?: string;
  helperText?: string;
}

const inputVariants = {
  variant: {
    default: 'border-gray-300 focus:border-blue-500 focus:ring-blue-500',
    error: 'border-red-500 focus:border-red-500 focus:ring-red-500',
  },
  size: {
    sm: 'px-3 py-1.5 text-sm',
    md: 'px-3 py-2 text-sm',
    lg: 'px-4 py-3 text-base',
  },
};

const getInputClasses = (
  variant: InputProps['variant'] = 'default',
  size: InputProps['size'] = 'md'
) => {
  return cn(
    'block w-full rounded-md border bg-white shadow-sm transition-colors duration-200',
    'focus:outline-none focus:ring-2 focus:ring-offset-0',
    'disabled:bg-gray-50 disabled:text-gray-500 disabled:cursor-not-allowed',
    'placeholder:text-gray-400',
    inputVariants.variant[variant],
    inputVariants.size[size]
  );
};

const Input = forwardRef<HTMLInputElement, InputProps>(
  (
    {
      variant = 'default',
      size = 'md',
      leftIcon,
      rightIcon,
      label,
      error,
      helperText,
      className,
      id,
      ...props
    },
    ref
  ) => {
    const inputId = id || `input-${Math.random().toString(36).substr(2, 9)}`;
    const hasError = Boolean(error);
    const currentVariant = hasError ? 'error' : variant;

    return (
      <div className="w-full">
        {label && (
          <label
            htmlFor={inputId}
            className="block text-sm font-medium text-gray-700 mb-1"
          >
            {label}
          </label>
        )}

        <div className="relative">
          {leftIcon && (
            <div className="absolute inset-y-0 left-0 pl-3 flex items-center pointer-events-none">
              <span className="text-gray-400 text-sm">{leftIcon}</span>
            </div>
          )}

          <input
            ref={ref}
            id={inputId}
            className={cn(
              getInputClasses(currentVariant, size),
              leftIcon && 'pl-10',
              rightIcon && 'pr-10',
              className
            )}
            {...props}
          />

          {rightIcon && (
            <div className="absolute inset-y-0 right-0 pr-3 flex items-center pointer-events-none">
              <span className="text-gray-400 text-sm">{rightIcon}</span>
            </div>
          )}
        </div>

        {(error || helperText) && (
          <p
            className={cn(
              'mt-1 text-xs',
              error ? 'text-red-600' : 'text-gray-500'
            )}
          >
            {error || helperText}
          </p>
        )}
      </div>
    );
  }
);

Input.displayName = 'Input';

export default Input;
