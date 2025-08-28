export interface UserProfile {
  id: string;
  name: string;
  email: string;
  organization: string;
}

export type UpdateUserProfile = Omit<UserProfile, 'id' | 'organization'>;

export interface User extends UserProfile {
  role: string;
  status: 'Active' | 'Inactive';
}

export type NewUser = Omit<User, 'id' | 'organization'>;
export type UpdateUser = Partial<NewUser>;
