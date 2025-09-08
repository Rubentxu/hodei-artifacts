const { chromium } = require('playwright');

async function testApplication() {
  console.log('🧪 Iniciando pruebas E2E con Playwright...');
  
  // Lanzar el navegador
  const browser = await chromium.launch({
    headless: false, // Ver el navegador en acción
    slowMo: 1000, // Ralentizar para ver las acciones
  });
  
  const context = await browser.newContext({
    viewport: { width: 1280, height: 720 }
  });
  
  const page = await context.newPage();
  
  try {
    console.log('📍 Navegando a la aplicación...');
    await page.goto('http://localhost:5174');
    
    // Esperar a que la página cargue
    await page.waitForLoadState('networkidle');
    
    console.log('🔍 Verificando elementos principales...');
    
    // Verificar que el título esté presente
    const title = await page.title();
    console.log(`✅ Título de la página: ${title}`);
    
    // Verificar que el header esté presente
    const header = await page.locator('header').first();
    await header.waitFor({ state: 'visible' });
    console.log('✅ Header visible');
    
    // Verificar el título de la aplicación
    const appTitle = await page.locator('h1').first().textContent();
    console.log(`✅ Título de la aplicación: ${appTitle}`);
    
    // Verificar navegación
    const navLinks = await page.locator('nav a').allTextContents();
    console.log(`✅ Enlaces de navegación: ${navLinks.join(', ')}`);
    
    // Probar navegación a Repositories
    console.log('🔄 Navegando a Repositories...');
    await page.click('nav a:has-text("Repositories")');
    await page.waitForLoadState('networkidle');
    
    // Verificar que estamos en la página de repositorios
    const repositoriesTitle = await page.locator('h1').first().textContent();
    console.log(`✅ Página de repositorios: ${repositoriesTitle}`);
    
    // Probar navegación a Search
    console.log('🔍 Navegando a Search...');
    await page.click('nav a:has-text("Search")');
    await page.waitForLoadState('networkidle');
    
    // Verificar que estamos en la página de búsqueda
    const searchTitle = await page.locator('h1').first().textContent();
    console.log(`✅ Página de búsqueda: ${searchTitle}`);
    
    // Verificar que los componentes UI estén presentes
    console.log('🧩 Verificando componentes UI...');
    
    // Verificar botones
    const buttons = await page.locator('button').count();
    console.log(`✅ Número de botones: ${buttons}`);
    
    // Verificar tarjetas
    const cards = await page.locator('.card, [class*="card"]').count();
    console.log(`✅ Número de tarjetas: ${cards}`);
    
    // Verificar inputs
    const inputs = await page.locator('input').count();
    console.log(`✅ Número de inputs: ${inputs}`);
    
    // Tomar captura de pantalla
    await page.screenshot({ path: 'test-results/frontend-test.png', fullPage: true });
    console.log('📸 Captura de pantalla guardada en test-results/frontend-test.png');
    
    console.log('🎉 ¡Pruebas E2E completadas exitosamente!');
    
  } catch (error) {
    console.error('❌ Error en las pruebas:', error);
    
    // Tomar captura de pantalla del error
    await page.screenshot({ path: 'test-results/error-screenshot.png', fullPage: true });
    console.log('📸 Captura de error guardada en test-results/error-screenshot.png');
    
    throw error;
  } finally {
    await browser.close();
    console.log('🔒 Navegador cerrado');
  }
}

// Ejecutar las pruebas
testApplication().catch(console.error);