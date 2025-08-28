export interface Policy {
  id: string;
  name: string;
  description: string;
  effect: 'Permit' | 'Forbid';
  body: string;
}

export type NewPolicy = Omit<Policy, 'id'>;
export type UpdatePolicy = Partial<NewPolicy>;
