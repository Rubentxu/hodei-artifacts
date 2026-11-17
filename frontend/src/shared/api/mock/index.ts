import { repositoryServiceMock } from './repositoryService.mock';
import { artifactServiceMock } from './artifactService.mock';
import { searchServiceMock } from './searchService.mock';
import { authServiceMock } from './authService.mock';

export {
  repositoryServiceMock,
  artifactServiceMock,
  searchServiceMock,
  authServiceMock,
};

// API Service Factory para usar los mocks
export const apiService = {
  repositories: repositoryServiceMock,
  artifacts: artifactServiceMock,
  search: searchServiceMock,
  auth: authServiceMock,
};
