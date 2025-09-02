import { test, expect } from '@playwright/test';

test.describe('Error Handling User Flow', () => { // Changed title
  test.beforeEach(async ({ page }) => {
    // Configurar estado inicial para cada test
    await page.goto('/');
  });

  test('should handle error scenarios gracefully', async ({ page }) => {
    // Test de manejo de errores
    
    // 1. Intentar subir archivo vacío
    await page.goto('/upload');
    await page.click('[data-testid="upload-button"]');
    await expect(page.locator('[data-testid="validation-errors"]')).toBeVisible();
    await expect(page.locator('[data-testid="validation-errors"]')).toContainText('File is required');

    // 2. Intentar acceder a artefacto inexistente
    await page.goto('/artifacts/non-existent-id/download');
    await expect(page.locator('[data-testid="not-found"]')).toBeVisible();
    await expect(page.locator('[data-testid="error-message"]')).toContainText('Artifact not found');

    // 3. Verificar conectividad de API
    const response = await page.request.get('/health');
    expect(response.ok()).toBeTruthy();
    
    const healthData = await response.json();
    expect(healthData.status).toBe('healthy');
  });

  test('should display 404 Not Found page for non-existent routes', async ({ page }) => {
    await page.goto('/non-existent-route');
    await expect(page.locator('h1')).toContainText('404');
    await expect(page.locator('h2')).toContainText('Page not found');
    await expect(page.locator('button:has-text("Go back")')).toBeVisible();
  });

  test('should display 403 Access Denied page', async ({ page }) => {
    // Simulate a 403 response or navigate to a known forbidden route
    // For now, we'll directly navigate to the page
    await page.goto('/unauthorized');
    await expect(page.locator('h1')).toContainText('403');
    await expect(page.locator('h2')).toContainText('Access Denied');
    await expect(page.locator('button:has-text("Go back")')).toBeVisible();
  });

  test('should display 500 Internal Server Error page', async ({ page }) => {
    // Simulate a 500 response or navigate to a known server error route
    // For now, we'll directly navigate to the page
    await page.goto('/server-error');
    await expect(page.locator('h1')).toContainText('500');
    await expect(page.locator('h2')).toContainText('Internal Server Error');
    await expect(page.locator('button:has-text("Refresh Page")')).toBeVisible();
  });
});