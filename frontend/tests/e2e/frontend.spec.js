import { test, expect } from '@playwright/test';

test.describe('Hodei Artifacts Frontend', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('http://localhost:5174');
  });

  test('should display the main page correctly', async ({ page }) => {
    // Verificar título de la página
    await expect(page).toHaveTitle(/Vite \+ React \+ TS/);
    
    // Verificar que el header esté presente
    const header = page.locator('header');
    await expect(header).toBeVisible();
    
    // Verificar el título de la aplicación
    const appTitle = page.locator('h1').first();
    await expect(appTitle).toContainText('Hodei Artifacts');
    
    // Verificar enlaces de navegación
    const navLinks = page.locator('nav a');
    await expect(navLinks).toHaveCount(3);
    await expect(navLinks.first()).toContainText('Dashboard');
    await expect(navLinks.nth(1)).toContainText('Repositories');
    await expect(navLinks.nth(2)).toContainText('Search');
  });

  test('should navigate to Repositories page', async ({ page }) => {
    // Hacer clic en Repositories
    await page.click('nav a:has-text("Repositories")');
    
    // Verificar que estamos en la página de repositorios
    await expect(page.locator('h1').first()).toContainText('Repositories');
    
    // Verificar elementos de la página
    await expect(page.locator('input[placeholder*="Search"]')).toBeVisible();
    await expect(page.locator('button:has-text("New Repository")')).toBeVisible();
  });

  test('should navigate to Search page', async ({ page }) => {
    // Hacer clic en Search
    await page.click('nav a:has-text("Search")');
    
    // Verificar que estamos en la página de búsqueda
    await expect(page.locator('h1').first()).toContainText('Search');
    
    // Verificar elementos de búsqueda
    await expect(page.locator('input[placeholder*="Search"]')).toBeVisible();
    await expect(page.locator('button:has-text("Search")')).toBeVisible();
  });

  test('should display UI components correctly', async ({ page }) => {
    // Verificar que hay botones
    const buttons = page.locator('button');
    await expect(buttons.first()).toBeVisible();
    
    // Verificar que hay inputs
    const inputs = page.locator('input');
    await expect(inputs.first()).toBeVisible();
    
    // Verificar que hay elementos con clases de Tailwind
    const tailwindElements = page.locator('[class*="bg-"], [class*="text-"], [class*="p-"]');
    await expect(tailwindElements.first()).toBeVisible();
  });

  test('should have responsive design', async ({ page }) => {
    // Verificar que el layout es responsive
    const container = page.locator('.container, [class*="container"]');
    await expect(container.first()).toBeVisible();
    
    // Verificar que hay elementos con clases de grid
    const gridElements = page.locator('[class*="grid"], [class*="flex"]');
    await expect(gridElements.first()).toBeVisible();
  });

  test('should display dashboard statistics', async ({ page }) => {
    // Verificar que hay estadísticas en el dashboard
    const stats = page.locator('[class*="text-2xl"], [class*="font-bold"]');
    await expect(stats.first()).toBeVisible();
    
    // Verificar que hay tarjetas de estadísticas
    const statCards = page.locator('[class*="bg-"], [class*="rounded"]').filter({ hasText: /Total|Active|Storage/ });
    await expect(statCards.first()).toBeVisible();
  });

  test('should handle navigation without errors', async ({ page }) => {
    // Navegar a diferentes páginas y verificar que no hay errores
    const pages = ['Repositories', 'Search'];
    
    for (const pageName of pages) {
      await page.click(`nav a:has-text("${pageName}")`);
      await expect(page.locator('h1').first()).toContainText(pageName);
      
      // Verificar que no hay mensajes de error
      await expect(page.locator('text=Error')).not.toBeVisible();
      await expect(page.locator('text=404')).not.toBeVisible();
    }
  });
});