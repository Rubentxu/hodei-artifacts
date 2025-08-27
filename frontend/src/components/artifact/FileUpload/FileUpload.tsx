import { useState, useCallback, useRef } from 'react';
import { Card } from '@/components/ui/Card';
import { Button } from '@/components/ui/Button';
import { Spinner } from '@/components/ui/Spinner';
import type { ApiError } from '@/shared/types';

interface FileUploadProps {
  repositoryId: string;
  path?: string;
  onUploadComplete?: (artifact: any) => void;
  onUploadError?: (error: ApiError) => void;
  acceptedTypes?: string[];
  maxSize?: number; // in bytes
  multiple?: boolean;
  className?: string;
}

interface UploadFile {
  id: string;
  file: File;
  progress: number;
  status: 'pending' | 'uploading' | 'completed' | 'error';
  error?: string;
}

export const FileUpload = ({
  repositoryId,
  path = '/',
  onUploadComplete,
  onUploadError,
  acceptedTypes = ['*/*'],
  maxSize = 100 * 1024 * 1024, // 100MB default
  multiple = true,
  className = '',
}: FileUploadProps) => {
  const [files, setFiles] = useState<UploadFile[]>([]);
  const [isDragging, setIsDragging] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const validateFile = useCallback((file: File): { valid: boolean; error?: string } => {
    // Check file size
    if (file.size > maxSize) {
      return {
        valid: false,
        error: `File size exceeds maximum allowed size of ${formatFileSize(maxSize)}`,
      };
    }

    // Check file type if specific types are specified
    if (acceptedTypes[0] !== '*/*') {
      const isValidType = acceptedTypes.some(type => {
        if (type.endsWith('/*')) {
          // Wildcard MIME type (e.g., image/*)
          const category = type.split('/')[0];
          return file.type.startsWith(`${category}/`);
        }
        return file.type === type;
      });

      if (!isValidType) {
        return {
          valid: false,
          error: `File type not allowed. Accepted types: ${acceptedTypes.join(', ')}`,
        };
      }
    }

    return { valid: true };
  }, [acceptedTypes, maxSize]);

  const handleFiles = useCallback((newFiles: FileList | File[]) => {
    const fileArray = Array.from(newFiles);
    const validFiles: UploadFile[] = [];

    fileArray.forEach(file => {
      const validation = validateFile(file);
      if (validation.valid) {
        validFiles.push({
          id: `${Date.now()}-${file.name}`,
          file,
          progress: 0,
          status: 'pending',
        });
      } else {
        // Show error for invalid files
        onUploadError?.({
          message: validation.error!,
          code: 'VALIDATION_ERROR',
        });
      }
    });

    if (validFiles.length > 0) {
      setFiles(prev => [...prev, ...validFiles]);
    }
  }, [validateFile, onUploadError]);

  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragging(true);
  }, []);

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragging(false);
  }, []);

  const handleDrop = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragging(false);

    const droppedFiles = e.dataTransfer.files;
    if (droppedFiles.length > 0) {
      handleFiles(droppedFiles);
    }
  }, [handleFiles]);

  const handleFileInput = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    const selectedFiles = e.target.files;
    if (selectedFiles && selectedFiles.length > 0) {
      handleFiles(selectedFiles);
      // Reset input to allow selecting same files again
      if (fileInputRef.current) {
        fileInputRef.current.value = '';
      }
    }
  }, [handleFiles]);

  const uploadFile = useCallback(async (uploadFile: UploadFile) => {
    const { artifactsApi } = await import('@/shared/api/artifacts');
    
    setFiles(prev => prev.map(f => 
      f.id === uploadFile.id ? { ...f, status: 'uploading' } : f
    ));

    try {
      const response = await artifactsApi.uploadArtifact(
        repositoryId,
        path,
        uploadFile.file,
        undefined,
        (progress) => {
          setFiles(prev => prev.map(f => 
            f.id === uploadFile.id ? { ...f, progress } : f
          ));
        }
      );

      setFiles(prev => prev.map(f => 
        f.id === uploadFile.id ? { ...f, status: 'completed', progress: 100 } : f
      ));

      onUploadComplete?.(response.data.artifact);
    } catch (error) {
      const apiError = error as ApiError;
      setFiles(prev => prev.map(f => 
        f.id === uploadFile.id ? { 
          ...f, 
          status: 'error', 
          error: apiError.message 
        } : f
      ));
      onUploadError?.(apiError);
    }
  }, [repositoryId, path, onUploadComplete, onUploadError]);

  const startUploads = useCallback(() => {
    const pendingFiles = files.filter(f => f.status === 'pending');
    pendingFiles.forEach(uploadFile);
  }, [files, uploadFile]);

  const removeFile = useCallback((fileId: string) => {
    setFiles(prev => prev.filter(f => f.id !== fileId));
  }, []);

  const formatFileSize = (bytes: number): string => {
    if (bytes === 0) return '0 B';
    
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    
    return `${parseFloat((bytes / Math.pow(k, i)).toFixed(2))} ${sizes[i]}`;
  };

  const getAcceptString = (): string => {
    return acceptedTypes.join(',');
  };

  const hasPendingFiles = files.some(f => f.status === 'pending');
  const hasActiveUploads = files.some(f => f.status === 'uploading');
  const completedCount = files.filter(f => f.status === 'completed').length;
  const errorCount = files.filter(f => f.status === 'error').length;

  return (
    <div className={className}>
      {/* Drop zone */}
      <Card
        className={`
          p-8 border-2 border-dashed transition-colors duration-200
          ${isDragging 
            ? 'border-blue-400 bg-blue-50' 
            : 'border-gray-300 hover:border-gray-400'
          }
        `}
        onDragOver={handleDragOver}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
        onClick={() => fileInputRef.current?.click()}
      >
        <div className="text-center">
          <div className="mx-auto w-12 h-12 mb-4">
            <svg
              className="w-full h-full text-gray-400"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d="M7 16a4 4 0 01-.88-7.903A5 5 0 1115.9 6L16 6a5 5 0 011 9.9M15 13l-3-3m0 0l-3 3m3-3v12"
              />
            </svg>
          </div>
          
          <p className="text-lg font-medium text-gray-900 mb-2">
            Drop files here or click to browse
          </p>
          
          <p className="text-sm text-gray-500 mb-4">
            {acceptedTypes[0] === '*/*' 
              ? `Maximum file size: ${formatFileSize(maxSize)}`
              : `Accepted types: ${acceptedTypes.join(', ')} • Max size: ${formatFileSize(maxSize)}`
            }
          </p>
          
          <Button
            type="button"
            variant="outline"
            onClick={(e) => {
              e.stopPropagation();
              fileInputRef.current?.click();
            }}
          >
            Select Files
          </Button>
          
          <input
            ref={fileInputRef}
            type="file"
            multiple={multiple}
            accept={getAcceptString()}
            onChange={handleFileInput}
            className="hidden"
          />
        </div>
      </Card>

      {/* File list */}
      {files.length > 0 && (
        <div className="mt-6 space-y-3">
          <div className="flex items-center justify-between">
            <h3 className="text-lg font-medium text-gray-900">
              Files to upload ({files.length})
            </h3>
            
            {hasPendingFiles && !hasActiveUploads && (
              <Button onClick={startUploads}>
                Start Upload
              </Button>
            )}
            
            {(completedCount > 0 || errorCount > 0) && (
              <div className="text-sm text-gray-500">
                {completedCount > 0 && <span className="text-green-600">{completedCount} completed</span>}
                {completedCount > 0 && errorCount > 0 && <span> • </span>}
                {errorCount > 0 && <span className="text-red-600">{errorCount} failed</span>}
              </div>
            )}
          </div>

          {files.map((file) => (
            <Card key={file.id} className="p-4">
              <div className="flex items-center justify-between">
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-3 mb-2">
                    <div className="flex-shrink-0">
                      {file.status === 'uploading' ? (
                        <Spinner size="sm" />
                      ) : file.status === 'completed' ? (
                        <div className="w-5 h-5 text-green-500">
                          <svg fill="currentColor" viewBox="0 0 20 20">
                            <path fillRule="evenodd" d="M16.707 5.293a1 1 0 010 1.414l-8 8a1 1 0 01-1.414 0l-4-4a1 1 0 011.414-1.414L8 12.586l7.293-7.293a1 1 0 011.414 0z" clipRule="evenodd" />
                          </svg>
                        </div>
                      ) : file.status === 'error' ? (
                        <div className="w-5 h-5 text-red-500">
                          <svg fill="currentColor" viewBox="0 0 20 20">
                            <path fillRule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zM8.707 7.293a1 1 0 00-1.414 1.414L8.586 10l-1.293 1.293a1 1 0 101.414 1.414L10 11.414l1.293 1.293a1 1 0 001.414-1.414L11.414 10l1.293-1.293a1 1 0 00-1.414-1.414L10 8.586 8.707 7.293z" clipRule="evenodd" />
                          </svg>
                        </div>
                      ) : (
                        <div className="w-5 h-5 text-gray-400">
                          <svg fill="currentColor" viewBox="0 0 20 20">
                            <path fillRule="evenodd" d="M4 4a2 2 0 012-2h4.586A2 2 0 0112 2.586L15.414 6A2 2 0 0116 7.414V16a2 2 0 01-2 2H6a2 2 0 01-2-2V4z" clipRule="evenodd" />
                          </svg>
                        </div>
                      )}
                    </div>
                    
                    <div className="min-w-0 flex-1">
                      <p className="text-sm font-medium text-gray-900 truncate">
                        {file.file.name}
                      </p>
                      <p className="text-xs text-gray-500">
                        {formatFileSize(file.file.size)}
                      </p>
                    </div>
                  </div>

                  {/* Progress bar */}
                  {file.status === 'uploading' && (
                    <div className="w-full bg-gray-200 rounded-full h-2">
                      <div
                        className="bg-blue-600 h-2 rounded-full transition-all duration-300"
                        style={{ width: `${file.progress}%` }}
                      />
                    </div>
                  )}

                  {/* Error message */}
                  {file.status === 'error' && file.error && (
                    <p className="text-sm text-red-600 mt-1">{file.error}</p>
                  )}
                </div>

                {/* Actions */}
                {file.status !== 'uploading' && (
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      removeFile(file.id);
                    }}
                    className="ml-4 text-gray-400 hover:text-gray-600 transition-colors"
                  >
                    <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
                    </svg>
                  </button>
                )}
              </div>
            </Card>
          ))}
        </div>
      )}
    </div>
  );
};