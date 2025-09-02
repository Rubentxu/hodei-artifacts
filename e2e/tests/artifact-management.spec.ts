import { test, expect } from '@playwright/test';

test.describe('Artifact Management User Flow', () => {
  test.beforeEach(async ({ page }) => {
    // Assuming user is logged in
    await page.goto('/'); // Or a specific artifact management page if it exists
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

  // Add more tests for artifact details, batch operations, etc.
});
