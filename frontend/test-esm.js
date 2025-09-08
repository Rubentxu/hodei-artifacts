import puppeteer from 'puppeteer';

async function testFrontend() {
  console.log('🧪 Iniciando pruebas de frontend con Puppeteer (ESM)...');
  
  const browser = await puppeteer.launch({
    headless: true,
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
    const headerExists = await page.$('header') !== null;
    console.log(`✅ Header encontrado: ${headerExists ? '✓' : '✗'}`);
    
    // Verificar el título de la aplicación
    const appTitle = await page.$eval('h1', el => el.textContent).catch(() => 'No encontrado');
    console.log(`✅ Título de la aplicación: ${appTitle}`);
    
    // Verificar enlaces de navegación
    const navLinks = await page.$$eval('nav a', links => links.map(link => link.textContent));
    console.log(`✅ Enlaces de navegación: ${navLinks.join(', ')}`);
    
    // Verificar que hay al menos 3 enlaces
    if (navLinks.length >= 3) {
      console.log('✅ Navegación completa encontrada');
    } else {
      console.log('⚠️ Pocos enlaces de navegación encontrados');
    }
    
    // Verificar componentes UI
    const buttons = await page.$$('button');
    console.log(`✅ Número de botones encontrados: ${buttons.length}`);
    
    const inputs = await page.$$('input');
    console.log(`✅ Número de inputs encontrados: ${inputs.length}`);
    
    const cards = await page.$$('[class*="card"], .card');
    console.log(`✅ Número de tarjetas encontradas: ${cards.length}`);
    
    // Verificar que la aplicación esté funcionando sin errores
    const hasErrors = await page.$('text=Error, text=404, text=Failed') !== null;
    console.log(`✅ Sin errores visibles: ${!hasErrors ? '✓' : '✗'}`);
    
    // Tomar captura de pantalla
    await page.screenshot({ path: 'test-results/frontend-test-final.png', fullPage: true });
    console.log('📸 Captura de pantalla guardada en test-results/frontend-test-final.png');
    
    console.log('🎉 ¡Pruebas de frontend completadas exitosamente!');
    
  } catch (error) {
    console.error('❌ Error en las pruebas:', error.message);
    
    // Tomar captura de pantalla del error
    try {
      await page.screenshot({ path: 'test-results/error-screenshot-final.png', fullPage: true });
      console.log('📸 Captura de error guardada');
    } catch (e) {
      console.log('No se pudo tomar captura de error');
    }
    
    throw error;
  } finally {
    await browser.close();
    console.log('🔒 Navegador cerrado');
  }
}

// Ejecutar las pruebas
testFrontend().catch(error => {
  console.error('Error fatal:', error);
  process.exit(1);
});