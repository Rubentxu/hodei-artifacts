import { test, expect } from '@playwright/test';

test.describe('Artifact Management E2E', () => {
  test.beforeEach(async ({ page }) => {
    // Configurar estado inicial para cada test
    await page.goto('/');
  });

  test('should upload and download an artifact', async ({ page }) => {
    // Test de flujo completo: subida y descarga de artefacto
    
    // 1. Navegar a la página de subida
    await page.click('[data-testid="upload-artifact"]');
    await expect(page.locator('h1')).toContainText('Upload Artifact');

    // 2. Llenar formulario de subida
    await page.fill('[data-testid="repository-id"]', 'test-repo-123');
    await page.fill('[data-testid="artifact-version"]', '1.0.0');
    await page.fill('[data-testid="file-name"]', 'test-artifact.jar');
    
    // 3. Seleccionar archivo para subir
    const fileInput = page.locator('[data-testid="file-input"]');
    await fileInput.setInputFiles({
      name: 'test-artifact.jar',
      mimeType: 'application/java-archive',
      buffer: Buffer.from('test file content')
    });

    // 4. Configurar coordenadas del paquete
    await page.selectOption('[data-testid="ecosystem"]', 'maven');
    await page.fill('[data-testid="namespace"]', 'com.example');
    await page.fill('[data-testid="package-name"]', 'test-artifact');

    // 5. Subir artefacto
    await page.click('[data-testid="upload-button"]');
    
    // 6. Verificar mensaje de éxito
    await expect(page.locator('[data-testid="upload-success"]')).toBeVisible();
    
    // 7. Capturar ID del artefacto desde la respuesta
    const artifactId = await page.locator('[data-testid="artifact-id"]').textContent();
    expect(artifactId).toBeTruthy();

    // 8. Navegar a descarga
    await page.goto(`/artifacts/${artifactId}/download`);
    
    // 9. Verificar página de descarga
    await expect(page.locator('[data-testid="download-info"]')).toBeVisible();
    await expect(page.locator('[data-testid="artifact-name"]')).toContainText('test-artifact.jar');
    
    // 10. Iniciar descarga
    const downloadPromise = page.waitForDownload();
    await page.click('[data-testid="download-button"]');
    const download = await downloadPromise;
    
    // 11. Verificar descarga
    expect(download.suggestedFilename()).toBe('test-artifact.jar');
  });

  test('should search for artifacts', async ({ page }) => {
    // Test de búsqueda de artefactos
    
    // 1. Navegar a búsqueda
    await page.goto('/search');
    await expect(page.locator('h1')).toContainText('Search Artifacts');

    // 2. Realizar búsqueda básica
    await page.fill('[data-testid="search-input"]', 'test-artifact');
    await page.click('[data-testid="search-button"]');

    // 3. Verificar resultados
    await expect(page.locator('[data-testid="search-results"]')).toBeVisible();
    
    // 4. Realizar búsqueda avanzada
    await page.click('[data-testid="advanced-search"]');
    await page.selectOption('[data-testid="ecosystem-filter"]', 'maven');
    await page.fill('[data-testid="namespace-filter"]', 'com.example');
    await page.click('[data-testid="advanced-search-button"]');

    // 5. Verificar filtros aplicados
    await expect(page.locator('[data-testid="active-filters"]')).toContainText('ecosystem: maven');
    await expect(page.locator('[data-testid="active-filters"]')).toContainText('namespace: com.example');
  });

  test('should handle repository management', async ({ page }) => {
    // Test de gestión de repositorios
    
    // 1. Navegar a repositorios
    await page.goto('/repositories');
    await expect(page.locator('h1')).toContainText('Repositories');

    // 2. Crear nuevo repositorio
    await page.click('[data-testid="create-repository"]');
    await page.fill('[data-testid="repository-name"]', 'test-repository');
    await page.fill('[data-testid="repository-description"]', 'Test repository for E2E');
    await page.selectOption('[data-testid="repository-type"]', 'maven');
    await page.click('[data-testid="create-button"]');

    // 3. Verificar repositorio creado
    await expect(page.locator('[data-testid="repository-created"]')).toBeVisible();
    
    // 4. Navegar a detalles del repositorio
    await page.click('[data-testid="repository-link"]');
    await expect(page.locator('[data-testid="repository-details"]')).toBeVisible();
    await expect(page.locator('[data-testid="repository-name"]')).toContainText('test-repository');

    // 5. Verificar listado de artefactos del repositorio
    await expect(page.locator('[data-testid="repository-artifacts"]')).toBeVisible();
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

  test('should support different package ecosystems', async ({ page }) => {
    // Test de soporte para diferentes ecosistemas
    
    const ecosystems = [
      { type: 'maven', namespace: 'com.example', name: 'test-lib', file: 'test-lib-1.0.0.jar' },
      { type: 'npm', namespace: '@scope', name: 'test-package', file: 'test-package-1.0.0.tgz' },
      { type: 'pypi', namespace: null, name: 'test-package', file: 'test_package-1.0.0.tar.gz' },
      { type: 'generic', namespace: 'custom', name: 'binary', file: 'binary-1.0.0.bin' }
    ];

    for (const ecosystem of ecosystems) {
      await page.goto('/upload');
      
      // Configurar ecosistema
      await page.selectOption('[data-testid="ecosystem"]', ecosystem.type);
      
      if (ecosystem.namespace) {
        await page.fill('[data-testid="namespace"]', ecosystem.namespace);
      }
      
      await page.fill('[data-testid="package-name"]', ecosystem.name);
      await page.fill('[data-testid="file-name"]', ecosystem.file);
      await page.fill('[data-testid="artifact-version"]', '1.0.0');
      
      // Verificar que el formulario se adapta al ecosistema
      if (ecosystem.type === 'maven') {
        await expect(page.locator('[data-testid="namespace"]')).toBeVisible();
      } else if (ecosystem.type === 'npm') {
        await expect(page.locator('[data-testid="namespace"]')).toBeVisible();
      }
      
      // Verificar formato canónico previsto
      const canonicalPreview = page.locator('[data-testid="canonical-preview"]');
      await expect(canonicalPreview).toContainText(ecosystem.type);
      await expect(canonicalPreview).toContainText(ecosystem.name);
    }
  });
});
