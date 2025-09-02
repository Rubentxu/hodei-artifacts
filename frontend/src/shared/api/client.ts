import type {
  AxiosInstance,
  AxiosRequestConfig,
  AxiosResponse,
  InternalAxiosRequestConfig,
} from 'axios';
import axios from 'axios';
import { useAuthStore } from '@/shared/stores/auth.store';
import type { ApiError } from '@/shared/types';

// Base API configuration
const BASE_URL =
  import.meta.env.VITE_API_BASE_URL || 'http://localhost:3000/api';

// Create axios instance
const apiClient: AxiosInstance = axios.create({
  baseURL: BASE_URL,
  timeout: 30000,
  headers: {
    'Content-Type': 'application/json',
  },
});

// Request interceptor to add auth token
apiClient.interceptors.request.use(
  (config: InternalAxiosRequestConfig): InternalAxiosRequestConfig => {
    const { token } = useAuthStore.getState();

    if (token) {
      config.headers.Authorization = `Bearer ${token}`;
    }

    return config;
  },
  error => {
    return Promise.reject(error);
  }
);

// Response interceptor for error handling
apiClient.interceptors.response.use(
  (response: AxiosResponse) => {
    return response;
  },
  error => {
    // Handle network errors
    if (!error.response) {
      const networkError: ApiError = {
        message: 'Network error. Please check your connection.',
        code: 'NETWORK_ERROR',
      };
      return Promise.reject(networkError);
    }

    const { status, data } = error.response;

    // Handle specific HTTP status codes
    switch (status) {
      case 401:
        // Unauthorized - clear auth and redirect to login
        useAuthStore.getState().logout();
        window.location.href = '/login';
        break;

      case 403:
        // Forbidden - redirect to unauthorized page
        window.location.href = '/unauthorized';
        return Promise.reject(error);

      case 404:
        // Not found - redirect to not-found page
        window.location.href = '/not-found';
        return Promise.reject(error);

      case 429:
        // Rate limited
        const rateLimitError: ApiError = {
          message:
            data?.message || 'Too many requests. Please try again later.',
          code: 'RATE_LIMITED',
          details: data?.details,
        };
        return Promise.reject(rateLimitError);

      case 500:
        // Server error - redirect to server-error page
        window.location.href = '/server-error';
        return Promise.reject(error);

      default:
        // Generic error
        const genericError: ApiError = {
          message: data?.message || 'An unexpected error occurred.',
          code: data?.code || 'UNKNOWN_ERROR',
          details: data?.details,
        };
        return Promise.reject(genericError);
    }

    return Promise.reject(error);
  }
);

// API service methods
export const apiService = {
  // GET request
  get: <T>(url: string, config?: AxiosRequestConfig): Promise<T> => {
    return apiClient.get(url, config).then(response => response.data);
  },

  // POST request
  post: <T>(
    url: string,
    data?: any,
    config?: AxiosRequestConfig
  ): Promise<T> => {
    return apiClient.post(url, data, config).then(response => response.data);
  },

  // PUT request
  put: <T>(
    url: string,
    data?: any,
    config?: AxiosRequestConfig
  ): Promise<T> => {
    return apiClient.put(url, data, config).then(response => response.data);
  },

  // PATCH request
  patch: <T>(
    url: string,
    data?: any,
    config?: AxiosRequestConfig
  ): Promise<T> => {
    return apiClient.patch(url, data, config).then(response => response.data);
  },

  // DELETE request
  delete: <T>(url: string, config?: AxiosRequestConfig): Promise<T> => {
    return apiClient.delete(url, config).then(response => response.data);
  },

  // Upload file with progress
  upload: <T>(
    url: string,
    file: File,
    onProgress?: (progress: number) => void,
    config?: AxiosRequestConfig
  ): Promise<T> => {
    const formData = new FormData();
    formData.append('file', file);

    return apiClient
      .post(url, formData, {
        ...config,
        headers: {
          'Content-Type': 'multipart/form-data',
        },
        onUploadProgress: progressEvent => {
          if (onProgress && progressEvent.total) {
            const progress = Math.round(
              (progressEvent.loaded * 100) / progressEvent.total
            );
            onProgress(progress);
          }
        },
      })
      .then(response => response.data);
  },

  // Download file
  download: (url: string, filename?: string): Promise<void> => {
    return apiClient
      .get(url, {
        responseType: 'blob',
      })
      .then(response => {
        const blob = new Blob([response.data]);
        const downloadUrl = window.URL.createObjectURL(blob);
        const link = document.createElement('a');
        link.href = downloadUrl;
        link.download = filename || 'download';
        document.body.appendChild(link);
        link.click();
        link.remove();
        window.URL.revokeObjectURL(downloadUrl);
      });
  },
};

// Export the raw axios instance for advanced usage
export { apiClient };

export default apiService;
