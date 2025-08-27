import { apiService } from './client';
import type { User, ApiResponse } from '@/shared/types';

export interface LoginRequest {
  email: string;
  password: string;
  rememberMe?: boolean;
}

export interface LoginResponse {
  user: User;
  token: string;
  refreshToken?: string;
}

export interface RegisterRequest {
  email: string;
  password: string;
  name: string;
  role?: 'admin' | 'user' | 'viewer';
}

export interface RefreshTokenRequest {
  refreshToken: string;
}

export interface RefreshTokenResponse {
  token: string;
  refreshToken?: string;
}

export const authApi = {
  // Login user
  login: async (credentials: LoginRequest): Promise<LoginResponse> => {
    return apiService.post<LoginResponse>('/auth/login', credentials);
  },

  // Register new user
  register: async (userData: RegisterRequest): Promise<ApiResponse<User>> => {
    return apiService.post<ApiResponse<User>>('/auth/register', userData);
  },

  // Logout user
  logout: async (): Promise<void> => {
    return apiService.post('/auth/logout');
  },

  // Refresh access token
  refreshToken: async (refreshToken: string): Promise<RefreshTokenResponse> => {
    return apiService.post<RefreshTokenResponse>('/auth/refresh', { refreshToken });
  },

  // Get current user profile
  getProfile: async (): Promise<ApiResponse<User>> => {
    return apiService.get<ApiResponse<User>>('/auth/profile');
  },

  // Update user profile
  updateProfile: async (userData: Partial<User>): Promise<ApiResponse<User>> => {
    return apiService.patch<ApiResponse<User>>('/auth/profile', userData);
  },

  // Change password
  changePassword: async (data: {
    currentPassword: string;
    newPassword: string;
  }): Promise<ApiResponse<void>> => {
    return apiService.post<ApiResponse<void>>('/auth/change-password', data);
  },

  // Request password reset
  forgotPassword: async (email: string): Promise<ApiResponse<void>> => {
    return apiService.post<ApiResponse<void>>('/auth/forgot-password', { email });
  },

  // Reset password with token
  resetPassword: async (data: {
    token: string;
    newPassword: string;
  }): Promise<ApiResponse<void>> => {
    return apiService.post<ApiResponse<void>>('/auth/reset-password', data);
  },

  // Verify email
  verifyEmail: async (token: string): Promise<ApiResponse<void>> => {
    return apiService.post<ApiResponse<void>>('/auth/verify-email', { token });
  },

  // Resend verification email
  resendVerification: async (email: string): Promise<ApiResponse<void>> => {
    return apiService.post<ApiResponse<void>>('/auth/resend-verification', { email });
  },
};