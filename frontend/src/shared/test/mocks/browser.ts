import { setupWorker } from 'msw/browser';
import { handlers } from './handlers';

// Setup requests interception for browser environment
export const worker = setupWorker(...handlers);
