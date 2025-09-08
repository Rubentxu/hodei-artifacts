const puppeteer = require('puppeteer');

async function testFrontend() {
  console.log('🧪 Iniciando pruebas de frontend con Puppeteer...');
  
  const browser = await puppeteer.launch({
    headless: false,
    slowMo: 100,
    args: ['--no-sandbox', '--disable-setuid-sandbox']
  });
  
  try {
    const page = await browser.newPage();
    
    console.log('📍 Navegando a http://localhost:5174...');
    await page.goto('http://localhost:5174', { waitUntil: 'networkidle2' });
    
    // Verificar título
    const title = await page.title();
    console.log(`✅ Título de la página: ${title}`);
    
    // Verificar que el header esté presente
    const headerText = await page.$eval('header', el => el.textContent);
    console.log(`✅ Header encontrado: ${headerText.includes('Hodei Artifacts') ? '✓' : '✗'}`);
    
    // Verificar enlaces de navegación
    const navLinks = await page.$$eval('nav a', links => links.map(link => link.textContent));
    console.log(`✅ Enlaces de navegación: ${navLinks.join(', ')}`);
    
    // Probar navegación a Repositories
    console.log('🔄 Navegando a Repositories...');
    await page.click('nav a:has-text("Repositories")');
    await page.waitForTimeout(1000);
    
    const repositoriesTitle = await page.$eval('h1', el => el.textContent);
    console.log(`✅ Página de repositorios: ${repositoriesTitle}`);
    
    // Probar navegación a Search
    console.log('🔍 Navegando a Search...');
    await page.click('nav a:has-text("Search")');
    await page.waitForTimeout(1000);
    
    const searchTitle = await page.$eval('h1', el => el.textContent);
    console.log(`✅ Página de búsqueda: ${searchTitle}`);
    
    // Verificar componentes UI
    const buttons = await page.$$('button');
    console.log(`✅ Número de botones encontrados: ${buttons.length}`);
    
    const inputs = await page.$$('input');
    console.log(`✅ Número de inputs encontrados: ${inputs.length}`);
    
    const cards = await page.$$('[class*="card"], .card');
    console.log(`✅ Número de tarjetas encontradas: ${cards.length}`);
    
    // Tomar captura de pantalla
    await page.screenshot({ path: 'test-results/frontend-test-puppeteer.png', fullPage: true });
    console.log('📸 Captura de pantalla guardada');
    
    console.log('🎉 ¡Pruebas de frontend completadas exitosamente!');
    
  } catch (error) {
    console.error('❌ Error en las pruebas:', error);
    throw error;
  } finally {
    await browser.close();
    console.log('🔒 Navegador cerrado');
  }
}

// Ejecutar las pruebas
testFrontend().catch(console.error);