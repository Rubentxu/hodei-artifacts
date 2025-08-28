export interface ApiToken {
  id: string;
  name: string;
  lastUsed: string;
  created: string;
  scopes: string[];
}

export type NewApiToken = {
  name: string;
  scopes: string[];
};

export type CreatedApiToken = ApiToken & {
  token: string; // The actual token, only shown on creation
};
