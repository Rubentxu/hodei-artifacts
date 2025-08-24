import { chromium, FullConfig } from '@playwright/test';

async function globalSetup(config: FullConfig) {
  console.log('🚀 Starting global setup for E2E tests...');
  
  // Aquí se pueden configurar servicios externos necesarios
  // Por ejemplo: iniciar contenedores Docker, configurar base de datos, etc.
  
  // Verificar que los servicios estén disponibles
  const browser = await chromium.launch();
  const page = await browser.newPage();
  
  try {
    // Esperar que el servicio esté disponible
    const baseURL = config.projects[0].use?.baseURL || 'http://localhost:3000';
    console.log(`Waiting for service at ${baseURL}/health...`);
    
    // Retry logic para esperar que el servicio esté listo
    let retries = 30;
    while (retries > 0) {
      try {
        await page.goto(`${baseURL}/health`, { timeout: 5000 });
        const response = await page.waitForResponse(`${baseURL}/health`, { timeout: 5000 });
        if (response.ok()) {
          console.log('✅ Service is ready!');
          break;
        }
      } catch (error) {
        retries--;
        if (retries === 0) {
          throw new Error(`Service not available after 30 retries: ${error}`);
        }
        console.log(`Retrying... (${retries} attempts left)`);
        await new Promise(resolve => setTimeout(resolve, 2000));
      }
    }
  } finally {
    await browser.close();
  }
  
  console.log('✅ Global setup completed');
}

export default globalSetup;
