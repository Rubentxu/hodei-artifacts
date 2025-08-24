async function globalTeardown() {
  console.log('🧹 Starting global teardown for E2E tests...');
  
  // Aquí se pueden limpiar recursos
  // Por ejemplo: parar contenedores Docker, limpiar datos de prueba, etc.
  
  console.log('✅ Global teardown completed');
}

export default globalTeardown;
