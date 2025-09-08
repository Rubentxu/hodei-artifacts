export { repositoryServiceMock } from './repositoryService.mock';
export { artifactServiceMock } from './artifactService.mock';
export { searchServiceMock } from './searchService.mock';
export { authServiceMock } from './authService.mock';

// API Service Factory para usar los mocks
export const apiService = {
  repositories: repositoryServiceMock,
  artifacts: artifactServiceMock,
  search: searchServiceMock,
  auth: authServiceMock,
};
